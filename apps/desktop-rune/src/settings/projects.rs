use super::{controls as c, request};
use crate::{
    App,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, row, text};
use serde_json::{Value, json};

/// `ProjectSettingsPanel` without `orchestration:operate`.
pub const CANNOT_CHANGE: &str = "This connection cannot change this project.";

pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T) -> El {
    let selected =
        s.ui.settings
            .project
            .as_ref()
            .and_then(|id| s.state().projects.iter().find(|p| p["id"] == *id));
    let Some(project) = selected else {
        return chooser(s, t);
    };
    // Web derives `isScratch` from the server's scratch root; the shell rows don't carry it.
    let mut project = project.clone();
    project["isScratch"] = json!(crate::shell::palette::actions::is_scratch_project(s, &project));
    let project = &project;
    let id = crate::data::s(project, "id").to_owned();
    let title = crate::data::s(project, "title").to_owned();
    let root = crate::data::s(project, "workspaceRoot").to_owned();
    let config = s.state().config.as_ref();
    let environment = config.and_then(|c| c["environment"]["label"].as_str()).unwrap_or("Environment").to_owned();
    // Older servers drop pin and archive, so their projects don't offer them.
    let organize = config.is_some_and(|c| c["environment"]["capabilities"]["projectOrganization"] == true);
    let key = |field: &str| format!("project::{id}::{field}");
    let operate = super::permissions::can(s, "orchestration:operate");
    let lock = || (!operate).then(|| CANNOT_CHANGE.to_owned());

    let mut overview = Vec::new();
    if !operate {
        overview.push(text(CANNOT_CHANGE).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().w_full().px(16.0).py(10.0));
    }
    overview.extend([
        name_row(s, t, project, &key("title"), lock()),
        super::permissions::gate("project-icon", super::project_icon::settings_row(s, t, project), lock()),
    ]);
    if organize {
        overview.push(c::setting_row(
            s,
            cx,
            t,
            &json!({"key": key("pinned"), "title": "Pin", "kind": "switch",
                "description": "Pinned projects come first in project pickers."}),
        ));
        let archived = !project["archivedAt"].is_null();
        let archive = key("archived");
        overview.push(super::about::about_row(
            t,
            "Archive",
            if archived {
                "Hidden from project pickers. Threads and files are kept."
            } else {
                "Hides the project from project pickers. Threads and files are kept."
            },
            super::permissions::gate(
                "project-archive",
                crate::kit::Button::new(if archived { "Unarchive" } else { "Archive" })
                    .variant("outline")
                    .size("sm")
                    .on_press(move |s: &mut App| super::update(s, &archive, json!(!archived)))
                    .build(t),
                lock(),
            ),
        ));
    }

    let page = col()
        .w_full()
        .gap(24.0)
        .child(
            row()
                .w_full()
                .px(14.0)
                .py(10.0)
                .gap(8.0)
                .radius(12.0)
                .border(1.0, t.c.info.alpha_mul(0.32))
                .fill(t.c.info.alpha_mul(0.04))
                .child(rune::icon(&crate::icons::INFO, 16.0).color(t.c.info).mt(2.0))
                .child(
                    text("Can't find a setting? Keep this project picked above and hop to any other settings page.")
                        .font(theme::TEXT_SM)
                        .color(t.c.foreground)
                        .wrap()
                        .grow(1.0)
                        .min_w(0.0),
                ),
        )
        .child(c::card(overview, t).key(super::title_key("Project overview")))
        .child(new_threads(s, cx, t))
        .child(actions(s, t, project));

    let checkout = super::about::about_row(t, &environment, &root, super::checkout_grouping::trigger(s, t, project));
    let remove = (id.clone(), title.clone());
    let danger = super::about::about_row(
        t,
        "Remove project",
        "Deletes the project entry and its threads. Files on disk are not touched.",
        super::permissions::gate(
            "project-remove",
            crate::kit::Button::new("Remove project")
                .variant("destructive-outline")
                .size("sm")
                .icon(&crate::icons::TRASH_2)
                .on_press(move |s: &mut App| ask_remove(s, &remove.0, &remove.1))
                .build(t),
            lock(),
        ),
    );
    let intro = text("Grouping rule decides how each checkout joins project groups in the sidebar. Changing it can move a checkout to a different project.")
        .font(theme::TEXT_XS)
        .color(t.c.muted_foreground)
        .wrap()
        .w_full()
        .px(16.0)
        .py(10.0);
    page.child(c::section("Checkouts", vec![intro, checkout], t))
        .child(c::section("Danger", vec![danger], t))
}

/// `SCRATCH_PROJECT_TITLE`: what a scratch project's name resets to.
const SCRATCH_TITLE: &str = "No project";

/// Web's Name row: the shared name, and Reset back to "No project" on a renamed scratch project.
fn name_row(s: &App, t: &T, project: &Value, key: &str, lock: Option<String>) -> El {
    let mut heading = row().gap(6.0).center_y().child(text("Name").key(super::title_key("Name")).font(theme::SM_MEDIUM).color(t.c.foreground));
    if project["isScratch"] == true && crate::data::s(project, "title") != SCRATCH_TITLE {
        let id = crate::data::s(project, "id").to_owned();
        heading = heading.child(
            crate::kit::Button::new("Reset project name")
                .variant("ghost-muted")
                .size("icon-micro")
                .icon(&crate::icons::UNDO_2)
                .disabled(lock.is_some())
                .on_press(move |s: &mut App| {
                    s.ui.settings.edits.remove(&format!("project::{id}::title"));
                    update(s, &id, "title", json!(SCRATCH_TITLE));
                })
                .build(t),
        );
    }
    let copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(heading)
        .child(
            text("The shared name for this project group in the sidebar and thread lists.")
                .font(theme::TEXT_XS.lh(18.0))
                .color(t.c.muted_foreground)
                .wrap(),
        );
    let input = super::permissions::gate(key, c::input(s, t, key, 256.0, "Project name", false), lock);
    let line = row().key(format!("row-{key}")).w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy).child(input);
    super::pulsed(s, t, "Name", line)
}

/// Web `ProjectDefaultsSettings` with category "project": the picked project's own model and
/// workspace for new threads, read from and written to its `projectSettingsOverrides` entry.
/// The fallback model is environment-wide, so it stays on Threads.
fn new_threads(s: &App, cx: &rune::Cx<App>, t: &T) -> El {
    let rows = [
        json!({"key": "defaultModelSelection", "title": "Model", "kind": "model", "options": [],
            "description": "Model for new threads in this project."}),
        json!({"key": "defaultThreadEnvMode", "title": "Workspace", "ariaLabel": "Default workspace", "kind": "select",
            "options": [["local", "Current checkout"], ["worktree", "New worktree"]],
            "description": "Where new threads in this project start."}),
    ];
    c::section("New threads", rows.iter().map(|spec| c::setting_row(s, cx, t, spec)).collect(), t)
}

/// Web's unscoped Projects page: every project as a row that picks it in the scope sentence.
fn chooser(s: &App, t: &T) -> El {
    let rows: Vec<El> = s
        .state()
        .projects
        .iter()
        .map(|project| {
            let id = crate::data::s(project, "id").to_owned();
            let title = crate::data::s(project, "title").to_owned();
            row()
                .w_full()
                .px(16.0)
                .py(12.0)
                .gap(12.0)
                .center_y()
                .focusable(true)
                .role("button")
                .label(title.clone())
                .child(crate::shell::sidebar::favicon::view(s, t, project, 16.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(
                            text(crate::data::s(project, "workspaceRoot").to_owned())
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground),
                        ),
                )
                .child(rune::icon(&crate::icons::CHEVRON_RIGHT, 16.0).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| {
                    super::commit_edits(s, true);
                    s.ui.settings.project = Some(id.clone());
                })
        })
        .collect();
    let mut page = col().w_full().gap(16.0).child(
        text("Choose a project to manage its name, icon, checkouts and actions.")
            .px(16.0)
            .font(theme::TEXT_SM)
            .color(t.c.muted_foreground),
    );
    if !rows.is_empty() {
        page = page.child(c::card(rows, t));
    }
    page
}

/// Subagents and side chats live inside their parent's view (`isListedThreadLineage`).
fn listed(thread: &Value) -> bool {
    thread["lineage"]["relationshipToParent"] != "subagent" && thread["lineage"]["sideChat"] != true
}

/// Web `ProjectActionsSettings`: the project's actions with their shortcuts, added and edited in
/// the same dialog the thread's scripts menu opens.
fn actions(s: &App, t: &T, project: &Value) -> El {
    use crate::chat::details::scripts;
    let id = crate::data::s(project, "id").to_owned();
    let add = id.clone();
    let imports = scripts::project_imports(s, project);
    let mut controls = row().gap(6.0).center_y();
    if imports.as_ref().is_some_and(|list| !list.is_empty()) {
        let (open, toggle) = (scripts::imports_open(s, &id), id.clone());
        controls = controls.child(
            row().key(scripts::IMPORT_ANCHOR).child(
                crate::kit::Button::new("Import scripts")
                    .variant("ghost")
                    .size("xs")
                    .on_press(move |s: &mut App| scripts::toggle_imports(s, &toggle))
                    .build(t)
                    .expanded(open),
            ),
        );
    }
    controls = controls.child(
        crate::kit::Button::new("Add action")
            .variant("outline")
            .size("xs")
            .icon(&crate::icons::PLUS)
            .on_press(move |s: &mut App| scripts::open_for_project(s, &add, None))
            .build(t)
            .key("project-add-action"),
    );
    let mut rows = vec![super::about::about_row(
        t,
        "Actions",
        "Commands that run in this project's checkout or its worktree, with optional shortcuts.",
        controls,
    )];
    let scripts = scripts::resolve(super::server_settings(s), project);
    if scripts.is_empty() {
        rows.push(
            text("No actions configured.")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground)
                .px(16.0)
                .py(8.0),
        );
    }
    for script in scripts {
        let name = crate::data::s(&script, "name").to_owned();
        let mut heading = row()
            .gap(8.0)
            .center_y()
            .min_w(0.0)
            .child(rune::icon(scripts::icon_for(crate::data::s(&script, "icon")), 16.0).color(t.c.muted_foreground))
            .child(text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate());
        for (on, label) in [
            (script["runOnWorktreeCreate"] == true, "setup"),
            (script["runOnSettle"] == true, "on settle"),
            (script["previewUrl"].as_str().is_some_and(|u| !u.is_empty()), "preview · desktop only"),
        ] {
            if on {
                heading = heading.child(
                    text(label)
                        .font(theme::TEXT_2XS)
                        .color(t.c.muted_foreground)
                        .px(6.0)
                        .py(1.0)
                        .radius(4.0)
                        .border(1.0, t.c.border.alpha_mul(0.6)),
                );
            }
        }
        let shortcut = crate::shell::palette::shortcut_label(s, &scripts::command_for(crate::data::s(&script, "id")));
        let mut control = row().gap(8.0).center_y();
        if !shortcut.is_empty() {
            control = control.child(text(shortcut).font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
        let (project, edit) = (id.clone(), script.clone());
        control = control.child(
            crate::kit::Button::new(format!("Edit {name}"))
                .variant("ghost-muted")
                .size("icon-xs")
                .icon(&crate::icons::SETTINGS)
                .on_press(move |s: &mut App| scripts::open_for_project(s, &project, Some(&edit)))
                .build(t),
        );
        rows.push(
            row()
                .w_full()
                .px(16.0)
                .py(8.0)
                .gap(32.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(heading)
                        .child(
                            text(crate::data::s(&script, "command").to_owned())
                                .font(theme::MONO_XS)
                                .color(t.c.muted_foreground)
                                .truncate(),
                        ),
                )
                .child(control),
        );
    }
    if imports.is_none() {
        rows.push(
            col()
                .w_full()
                .px(16.0)
                .py(12.0)
                .gap(4.0)
                .child(text("t3.json is invalid").font(theme::SM_MEDIUM).color(t.c.warning))
                .child(
                    text("A t3.json exists in this checkout but fails to parse, so every action and icon it declares is ignored. Check the JSON syntax and icon values.")
                        .font(theme::TEXT_XS.lh(18.0))
                        .color(t.c.warning)
                        .wrap(),
                ),
        );
    }
    c::section("Actions", rows, t)
}

/// Web `planProjectRemoval`'s confirmation for a whole single-checkout project: the question,
/// then the details.
pub fn removal_message(title: &str, root: &str, environment: Option<&str>, threads: usize) -> (String, String) {
    let question = match threads {
        0 => format!("Remove project \"{title}\"?"),
        1 => format!("Remove project \"{title}\" and delete its 1 thread?"),
        n => format!("Remove project \"{title}\" and delete its {n} threads?"),
    };
    let mut details = vec![format!("Path: {root}")];
    if let Some(environment) = environment {
        details.push(format!("Environment: {environment}"));
    }
    details.push(
        if threads > 0 {
            "Those threads and any archived threads are deleted from T3 Code. Provider sessions are kept."
        } else {
            "Any archived threads are deleted from T3 Code. Provider sessions are kept."
        }
        .to_owned(),
    );
    details.push("This removes only the project entries, not the files on disk.".to_owned());
    details.push("This action cannot be undone.".to_owned());
    (question, details.join("\n"))
}

/// `removeMembers`: web's destructive `dialogs.confirm` with `planProjectRemoval`'s message.
fn ask_remove(s: &mut App, id: &str, title: &str) {
    let project = s.state().projects.iter().find(|p| p["id"] == id);
    let root = project.map(|p| crate::data::s(p, "workspaceRoot")).unwrap_or("");
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str());
    let threads = s.state().threads.iter().filter(|thread| thread["projectId"] == id && listed(thread)).count();
    let (question, details) = removal_message(title, root, environment, threads);
    let id = id.to_owned();
    crate::shell::confirm::request(s, format!("{question}\n{details}"), true, move |s: &mut App| {
        request(s, "projects.mutate", json!({"type":"project.delete","commandId":s5_net::uuid(),"projectId":id,"force":true}));
        s.ui.settings.project = None;
    });
}
pub fn value(s: &App, id: &str, field: &str) -> Value {
    let Some(project) = s.state().projects.iter().find(|p| p["id"] == id) else {
        return Value::Null;
    };
    match field {
        // The shell carries timestamps; the update command takes booleans.
        "pinned" => json!(!project["pinnedAt"].is_null()),
        "archived" => json!(!project["archivedAt"].is_null()),
        _ => project[field].clone(),
    }
}
pub fn update(s: &mut App, id: &str, field: &str, value: Value) {
    if field == "title" && value.as_str().is_none_or(|v| v.trim().is_empty()) {
        crate::shell::palette::toast_warning(s, "Project title cannot be empty", None, vec![], false);
        return;
    }
    let value = if field == "faviconPath" && value == "" {
        Value::Null
    } else {
        value
    };
    request(
        s,
        "projects.mutate",
        json!({"type":"project.update","commandId":s5_net::uuid(),"projectId":id,field:value}),
    );
}
pub const ARCHIVED_SNAPSHOT: &str = "orchestration.getArchivedShellSnapshot";
/// The environment a row of the merged archive came from.
const ENV: &str = "_env";

/// The pending-request and result name of one environment's archive snapshot.
pub(super) fn archive_key(env: &str) -> String {
    format!("{ARCHIVED_SNAPSHOT}@{env}")
}

/// Web `useArchivedThreadSnapshots`: asks every environment for its archive.
pub(super) fn request_archives(s: &mut App) {
    for key in crate::env::keys(s) {
        if let Some(net) = crate::env::net_of(s, &key) {
            let id = net.request(ARCHIVED_SNAPSHOT, json!({}));
            s.ui.settings.pending.insert(id, archive_key(&key));
        }
    }
}

/// An environment's archive arrived: its threads' commands go back to that environment.
pub(super) fn archive_fetched(s: &mut App, method: &str, result: &Value) {
    let Some(env) = method.strip_prefix(ARCHIVED_SNAPSHOT).and_then(|m| m.strip_prefix('@')) else { return };
    for thread in result["threads"].as_array().into_iter().flatten() {
        crate::env::adopt(s, crate::data::s(thread, "id"), env);
    }
}

fn tagged(row: &Value, env: &str) -> Value {
    let mut row = row.clone();
    row[ENV] = json!(env);
    row
}

/// The shell stream's snapshot never carries archived threads, so the page fetches them and lets
/// this session's archive/restore events win over the fetched copy.
fn archived_rows(fetched: &[Value], live: &[Value], active: &[Value]) -> Vec<Value> {
    let mut rows: Vec<Value> = fetched
        .iter()
        .filter(|row| live.iter().all(|l| l["id"] != row["id"]))
        .chain(live)
        .filter(|row| active.iter().all(|a| a["id"] != row["id"]))
        .cloned()
        .collect();
    rows.sort_by(|a, b| crate::data::s(b, "archivedAt").cmp(crate::data::s(a, "archivedAt")));
    rows
}

/// Web `ArchivedThreadsPanel`'s groups: each project with archived threads, in project order,
/// its threads newest-archived first (then by id, as web's `localeCompare` tiebreak).
/// Each project's archived threads, newest first. With `logical`, projects it keys alike (one
/// repository checked out on several machines) share a group titled by all of them, as web merges
/// the scope's logical project groups once archives span environments.
fn archive_groups(projects: &[Value], threads: &[Value], logical: Option<&dyn Fn(&Value) -> String>) -> Vec<(Value, Vec<Value>)> {
    let key = |t: &Value| t["archivedAt"].as_str().or(t["createdAt"].as_str()).unwrap_or("").to_owned();
    let mut seen = std::collections::HashSet::new();
    let projects: Vec<&Value> = projects.iter().filter(|p| seen.insert((crate::data::s(p, ENV).to_owned(), crate::data::s(p, "id").to_owned()))).collect();
    let group_of = |p: &Value| logical.map_or_else(|| format!("{}:{}", crate::data::s(p, ENV), crate::data::s(p, "id")), |f| f(p));
    let mut groups: Vec<(String, Value, Vec<Value>)> = vec![];
    for project in &projects {
        let rows = threads.iter().filter(|t| t["projectId"] == project["id"] && t[ENV] == project[ENV]).cloned();
        let group = group_of(project);
        match groups.iter_mut().find(|(g, ..)| *g == group) {
            Some((.., all)) => all.extend(rows),
            None => groups.push((group, (*project).clone(), rows.collect())),
        }
    }
    groups
        .into_iter()
        .filter(|(.., rows)| !rows.is_empty())
        .map(|(group, mut project, mut rows)| {
            rows.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| crate::data::s(b, "id").cmp(crate::data::s(a, "id"))));
            let members: Vec<&Value> = projects.iter().copied().filter(|p| group_of(p) == group).collect();
            if members.len() > 1 {
                project["title"] = json!(crate::shell::sidebar::folders::label(&members));
            }
            (project, rows)
        })
        .collect()
}

pub fn archived(s: &App, t: &T) -> El {
    let snapshots: Vec<(String, &Value)> =
        crate::env::keys(s).into_iter().filter_map(|key| s.ui.settings.results.get(&archive_key(&key)).map(|r| (key, r))).collect();
    let fetched: Vec<Value> = snapshots.iter().flat_map(|(key, r)| r["threads"].as_array().into_iter().flatten().map(|row| tagged(row, key))).collect();
    let live = |rows: &[Value]| rows.iter().map(|row| tagged(row, &crate::env::of(s, row))).collect::<Vec<_>>();
    // Each archive snapshot names its projects; live ones cover threads archived since the fetch.
    let projects: Vec<Value> = snapshots
        .iter()
        .flat_map(|(key, r)| r["projects"].as_array().into_iter().flatten().map(|row| tagged(row, key)))
        .chain(live(&s.state().projects))
        .filter(|p| s.ui.settings.project.as_ref().is_none_or(|id| p["id"] == *id))
        .collect();
    let threads = archived_rows(&fetched, &live(&s.state().archived), &s.state().threads);
    // `hasMultipleEnvironments`: rows then name their environment, and group by logical project.
    let several = threads
        .iter()
        .filter(|t| projects.iter().any(|p| p["id"] == t["projectId"] && p[ENV] == t[ENV]))
        .map(|row| crate::data::s(row, ENV))
        .collect::<std::collections::HashSet<_>>()
        .len()
        > 1;
    let grouping = crate::shell::sidebar::folders::Grouping::read(s);
    let logical = |p: &Value| {
        let mut p = p.clone();
        p[crate::env::TAG] = p[ENV].clone();
        grouping.key(&p)
    };
    let groups = archive_groups(&projects, &threads, several.then_some(&logical as &dyn Fn(&Value) -> String));
    if groups.is_empty() {
        let loading = s.ui.settings.pending.values().any(|m| m.starts_with(ARCHIVED_SNAPSHOT));
        let error = snapshots.iter().find_map(|(_, r)| r[super::LOAD_ERROR].as_str());
        let (glyph, title, description) = if loading {
            (crate::kit::Spinner::new().size("sm").tone("muted").build(&crate::kit::theme(t)), "Loading archived threads", "Checking connected environments.")
        } else if let Some(error) = error {
            (rune::icon(&crate::icons::ARCHIVE, 14.0).color(t.c.muted_foreground), "Could not load archived threads", error)
        } else {
            (rune::icon(&crate::icons::ARCHIVE, 14.0).color(t.c.muted_foreground), "No archived threads", "Archived threads will appear here.")
        };
        let row = col()
            .padding(16.0)
            .gap(4.0)
            .child(row().gap(8.0).center_y().child(glyph).child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground)))
            .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground));
        return c::section("Archived threads", vec![row], t);
    }
    let mut page = col().w_full().gap(24.0);
    for (project, threads) in &groups {
        let header = row()
            .w_full()
            .h(28.0)
            .px(16.0)
            .gap(8.0)
            .center_y()
            .child(crate::shell::sidebar::favicon::view(s, t, project, 16.0))
            .child(
                text(crate::data::s(project, "title").to_owned())
                    .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                    .color(t.c.foreground)
                    .truncate()
                    .min_w(0.0),
            );
        let rows = threads.iter().map(|thread| archived_row(s, t, thread, several)).collect();
        page = page.child(col().w_full().gap(10.0).child(header).child(c::card(rows, t)));
    }
    page
}

/// Web's archived `SettingsRow`: when it was archived and created, Unarchive, and a right-click
/// menu.
fn archived_row(s: &App, t: &T, thread: &Value, several: bool) -> El {
    let id = crate::data::s(thread, "id").to_owned();
    let title = crate::data::s(thread, "title").to_owned();
    let created = crate::data::s(thread, "createdAt");
    let archived_at = thread["archivedAt"].as_str().unwrap_or(created);
    let place = if several { format!("{} \u{b7} ", crate::env::label(s, crate::data::s(thread, ENV))) } else { String::new() };
    let description = format!(
        "{place}Archived {} \u{b7} Created {}",
        super::storage::relative(archived_at, s.now_ms),
        super::storage::relative(created, s.now_ms)
    );
    let restore = id.clone();
    let menu = (id, title.clone());
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground)),
        )
        .child(
            crate::kit::Button::new("Unarchive")
                .variant("outline")
                .size("sm")
                .icon(&crate::icons::ARCHIVE_X)
                .on_press(move |s: &mut App| unarchive(s, &restore))
                .build(t),
        )
        .on_secondary_press(move |s: &mut App, at: rune::PointerPress, _: &mut rune::Tx| {
            s.ui.settings.archive_menu = Some((menu.0.clone(), menu.1.clone(), at.x, at.y));
        })
}

fn unarchive(s: &mut App, id: &str) {
    dispatch(s, json!({"type": "thread.unarchive", "threadId": id}), UNARCHIVE);
}

const UNARCHIVE: &str = "thread action: unarchive";
const DELETE: &str = "thread action: delete";

/// Web's toast title when an archived thread's unarchive or delete command fails.
pub(super) fn failure_title(method: &str) -> Option<&'static str> {
    match method {
        UNARCHIVE => Some("Failed to unarchive thread"),
        DELETE => Some("Failed to delete thread"),
        _ => None,
    }
}

/// Web `useThreadActions.confirmAndDeleteThread`: asks first while `confirmThreadDelete` is on.
fn request_delete(s: &mut App, id: String, title: String) {
    // `threadOperationFailure`: without operate access it fails before asking.
    if !crate::shell::sidebar::grants_on(s, &crate::env::owner(s, &id), "orchestration:operate") {
        return crate::shell::palette::toast_error_brief(s, "Failed to delete thread", Some("This connection cannot change threads."));
    }
    let key = "confirmThreadDelete";
    let confirm = s.client_settings.values.get(key).or_else(|| super::client::defaults().get(key)).and_then(Value::as_bool).unwrap_or(false);
    if confirm {
        let question = format!("Delete thread \"{title}\"?\nThis permanently clears conversation history for this thread.");
        crate::shell::confirm::request(s, question, true, move |s: &mut App| dispatch(s, json!({"type": "thread.delete", "threadId": id}), DELETE));
    } else {
        dispatch(s, json!({"type": "thread.delete", "threadId": id}), DELETE);
    }
}

/// The archived thread's right-click menu: Unarchive, then Delete.
pub fn archive_menu(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, title, x, y)) = s.ui.settings.archive_menu.clone() else { return };
    let close = |s: &mut App| s.ui.settings.archive_menu = None;
    let restore = id.clone();
    crate::kit::Menu::new("archived-thread-menu")
        .native(true)
        .width(198.0)
        .open(true)
        .on_close(close)
        .at(x, y)
        .item(crate::kit::MenuItem::new("Unarchive").on_select(move |s: &mut App| {
            close(s);
            unarchive(s, &restore);
        }))
        .item(crate::kit::MenuItem::new("Delete").variant("destructive").on_select(move |s: &mut App| {
            close(s);
            request_delete(s, id.clone(), title.clone());
        }))
        .overlay(&crate::kit::theme(t), layers);
}
fn dispatch(s: &mut App, command: Value, action: &str) {
    if let Some(net) = crate::env::net_for(s, command["threadId"].as_str().unwrap_or_default()) {
        let id = net.dispatch(command);
        s.ui.settings.pending.insert(id, action.into());
    }
}
/// Scenes: the fixture's threads, archived under their projects, with the first one's right-click
/// menu open when `menu`.
pub(super) fn scene_archive(s: &mut App, menu: bool) {
    let threads: Vec<Value> = s
        .state()
        .threads
        .iter()
        .filter(|t| listed(t))
        .take(5)
        .enumerate()
        .map(|(i, t)| {
            let mut t = t.clone();
            t["id"] = json!(format!("archived-{i}"));
            t["archivedAt"] = t["updatedAt"].clone();
            t
        })
        .collect();
    if menu {
        if let Some(first) = threads.first() {
            s.ui.settings.archive_menu = Some((crate::data::s(first, "id").to_owned(), crate::data::s(first, "title").to_owned(), 720.0, 220.0));
        }
    }
    // Every project names its repository, so checkouts on the other machines merge with it.
    let projects: Vec<Value> = s
        .state()
        .projects
        .iter()
        .map(|p| {
            let mut p = p.clone();
            if p["repositoryIdentity"].is_null() {
                let name = crate::data::s(&p, "title").to_owned();
                p["repositoryIdentity"] = json!({"canonicalKey": format!("github.com/acme/{name}"), "name": name});
            }
            p
        })
        .collect();
    // Two more machines with checkouts of the first two projects.
    for (env, label, take) in [("box", "Build box", 2), ("lab", "Lab mini", 1)] {
        s.ui.env.saved.push(crate::env::Saved { id: env.into(), label: label.into(), base: format!("https://{env}.invalid"), bearer: String::new() });
        let copies: Vec<Value> = projects.iter().take(take).map(|p| {
            let mut p = p.clone();
            p["id"] = json!(format!("{env}-{}", crate::data::s(&p, "id")));
            p
        }).collect();
        let rows: Vec<Value> = threads
            .iter()
            .filter(|t| copies.iter().any(|p| crate::data::s(p, "id") == format!("{env}-{}", crate::data::s(t, "projectId"))))
            .map(|t| {
                let mut t = t.clone();
                t["id"] = json!(format!("{env}-{}", crate::data::s(&t, "id")));
                t["projectId"] = json!(format!("{env}-{}", crate::data::s(&t, "projectId")));
                t
            })
            .collect();
        s.ui.settings.results.insert(archive_key(env), json!({"projects": copies, "threads": rows}));
    }
    s.ui.settings.results.insert(archive_key(crate::env::LOCAL), json!({"projects": projects, "threads": threads}));
}

#[cfg(test)]
mod tests {
    use super::{archive_fetched, archive_groups, archive_key, archived_rows, removal_message, request_delete};
    use crate::App;
    use serde_json::json;

    #[test]
    fn removal_confirmation_matches_web_plan() {
        let (question, details) = removal_message("api", "/src/api", Some("Mac"), 2);
        assert_eq!(question, "Remove project \"api\" and delete its 2 threads?");
        assert_eq!(
            details,
            "Path: /src/api\nEnvironment: Mac\nThose threads and any archived threads are deleted from T3 Code. Provider sessions are kept.\nThis removes only the project entries, not the files on disk.\nThis action cannot be undone."
        );
        assert_eq!(removal_message("api", "/a", None, 0).0, "Remove project \"api\"?");
    }

    #[test]
    fn each_environments_archive_groups_under_its_own_projects_and_unarchives_there() {
        let projects = [json!({"id": "p", "title": "api", "_env": "local"}), json!({"id": "p", "title": "api", "_env": "box"})];
        let threads = [
            json!({"id": "a", "projectId": "p", "_env": "local", "archivedAt": "2026-11-02T00:00:00Z"}),
            json!({"id": "b", "projectId": "p", "_env": "box", "archivedAt": "2026-11-03T00:00:00Z"}),
        ];
        let groups = archive_groups(&projects, &threads, None);
        assert_eq!(groups.len(), 2, "the same project id on two machines stays two groups");
        assert_eq!(groups[1].1[0]["id"], "b");
        // Across environments the scope's logical groups merge checkouts of one repository.
        let repo = |id: &str, env: &str, title: &str| json!({"id": id, "title": title, "_env": env, "repositoryIdentity": {"canonicalKey": "github.com/acme/api", "name": "api"}});
        let projects = [repo("p", "", "api"), repo("q", "box", "api (box)"), json!({"id": "r", "title": "docs", "_env": "box"})];
        let threads = [
            json!({"id": "a", "projectId": "p", "_env": "", "archivedAt": "2026-11-02T00:00:00Z"}),
            json!({"id": "b", "projectId": "q", "_env": "box", "archivedAt": "2026-11-03T00:00:00Z"}),
            json!({"id": "c", "projectId": "r", "_env": "box", "archivedAt": "2026-11-04T00:00:00Z"}),
        ];
        let grouping = crate::shell::sidebar::folders::Grouping::read(&App::new());
        let logical = |p: &serde_json::Value| {
            let mut p = p.clone();
            p[crate::env::TAG] = p["_env"].clone();
            grouping.key(&p)
        };
        let groups = archive_groups(&projects, &threads, Some(&logical));
        let shape: Vec<(&str, Vec<&str>)> = groups.iter().map(|(p, rows)| (p["title"].as_str().unwrap(), rows.iter().map(|r| r["id"].as_str().unwrap()).collect())).collect();
        assert_eq!(shape, [("api", vec!["b", "a"]), ("docs", vec!["c"])], "one group per repository, titled for all its checkouts");
        let mut s = App::new();
        archive_fetched(&mut s, &archive_key("box"), &json!({"threads": [{"id": "b"}]}));
        assert_eq!(crate::env::owner(&s, "b"), "box");
    }

    #[test]
    fn archived_rows_merge_the_fetch_with_live_events() {
        let fetched = [
            json!({"id":"old","title":"fetched","archivedAt":"2026-11-01T00:00:00Z"}),
            json!({"id":"restored","archivedAt":"2026-11-02T00:00:00Z"}),
            json!({"id":"renamed","title":"stale","archivedAt":"2026-11-03T00:00:00Z"}),
        ];
        let live = [
            json!({"id":"renamed","title":"live","archivedAt":"2026-11-03T00:00:00Z"}),
            json!({"id":"new","archivedAt":"2026-11-04T00:00:00Z"}),
        ];
        let active = [json!({"id":"restored"})];
        let rows = archived_rows(&fetched, &live, &active);
        let ids: Vec<_> = rows.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["new", "renamed", "old"]);
        assert_eq!(rows[1]["title"], "live");
    }

    #[test]
    fn archived_threads_group_under_their_projects_like_web() {
        let projects = [json!({"id":"a","title":"api"}), json!({"id":"b","title":"web"}), json!({"id":"a"}), json!({"id":"c"})];
        let threads = [
            json!({"id":"1","projectId":"a","archivedAt":"2026-11-01T00:00:00Z","createdAt":"2026-10-01T00:00:00Z"}),
            json!({"id":"2","projectId":"b","archivedAt":null,"createdAt":"2026-11-05T00:00:00Z"}),
            json!({"id":"3","projectId":"a","archivedAt":"2026-11-03T00:00:00Z","createdAt":"2026-10-01T00:00:00Z"}),
            json!({"id":"4","projectId":"a","archivedAt":"2026-11-03T00:00:00Z","createdAt":"2026-10-01T00:00:00Z"}),
            json!({"id":"5","projectId":"gone","archivedAt":"2026-11-09T00:00:00Z"}),
        ];
        let groups = archive_groups(&projects, &threads, None);
        let shape: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|(p, rows)| (p["id"].as_str().unwrap(), rows.iter().map(|r| r["id"].as_str().unwrap()).collect()))
            .collect();
        // Project order, each project once, empty ones and orphans dropped; newest archived first,
        // ties by id descending, and a missing archivedAt falls back to createdAt.
        assert_eq!(shape, [("a", vec!["4", "3", "1"]), ("b", vec!["2"])]);
    }

    #[test]
    fn deleting_from_the_archive_asks_only_while_confirm_thread_delete_is_on() {
        let mut s = App::new();
        request_delete(&mut s, "t".into(), "Fix it".into());
        assert_eq!(
            crate::shell::confirm::current(&s),
            Some("Delete thread \"Fix it\"?\nThis permanently clears conversation history for this thread."),
            "on by default"
        );
        crate::shell::confirm::respond(&mut s, false);
        s.client_settings.values.insert("confirmThreadDelete".into(), json!(false));
        request_delete(&mut s, "t".into(), "Fix it".into());
        assert_eq!(crate::shell::confirm::current(&s), None);

        // Without operate access it fails at once, before any confirmation (threadOperationFailure).
        s.client_settings.values.insert("confirmThreadDelete".into(), json!(true));
        crate::chat::details::set_session(&mut s, Some(Ok(json!({"authenticated": true, "permissions": ["orchestration:read"]}))));
        request_delete(&mut s, "t".into(), "Fix it".into());
        assert_eq!(crate::shell::confirm::current(&s), None);
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Failed to delete thread"));
        assert_eq!(crate::shell::palette::toast_description(&s), Some("This connection cannot change threads."));
    }

    #[test]
    fn archive_action_failures_toast_web_titles() {
        assert_eq!(super::failure_title(super::UNARCHIVE), Some("Failed to unarchive thread"));
        assert_eq!(super::failure_title(super::DELETE), Some("Failed to delete thread"));
        assert_eq!(super::failure_title("server.updateSettings"), None);
    }
}
