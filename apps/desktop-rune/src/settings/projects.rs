use super::{controls as c, request};
use crate::{
    App,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

pub fn view(s: &App, cx: &mut rune::Cx<App>, t: &T) -> El {
    let selected =
        s.ui.settings
            .project
            .as_ref()
            .and_then(|id| s.state().projects.iter().find(|p| p["id"] == *id));
    let Some(project) = selected else {
        return chooser(s, t);
    };
    let id = crate::data::s(project, "id").to_owned();
    let title = crate::data::s(project, "title").to_owned();
    let root = crate::data::s(project, "workspaceRoot").to_owned();
    let config = s.state().config.as_ref();
    let environment = config.and_then(|c| c["environment"]["label"].as_str()).unwrap_or("Environment").to_owned();
    // Older servers drop pin and archive, so their projects don't offer them.
    let organize = config.is_some_and(|c| c["environment"]["capabilities"]["projectOrganization"] == true);
    let key = |field: &str| format!("project::{id}::{field}");

    let mut overview = vec![
        c::setting_row(
            s,
            cx,
            t,
            &json!({"key": key("title"), "title": "Name", "kind": "input", "width": 256, "placeholder": "Project name",
                "description": "The shared name for this project group in the sidebar and thread lists."}),
        ),
        super::project_icon::settings_row(t, project),
    ];
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
            crate::kit::Button::new(if archived { "Unarchive" } else { "Archive" })
                .variant("outline")
                .size("sm")
                .on_press(move |s: &mut App| super::update(s, &archive, json!(!archived)))
                .build(t),
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
        .child(c::card(overview, t))
        .child(new_threads(s, cx, t))
        .child(actions(s, t, project));

    let checkout = super::about::about_row(t, &environment, &root, el());
    let remove = (id.clone(), title.clone());
    let danger = super::about::about_row(
        t,
        "Remove project",
        "Deletes the project entry and its threads. Files on disk are not touched.",
        crate::kit::Button::new("Remove project")
            .variant("destructive-outline")
            .size("sm")
            .icon(&crate::icons::TRASH_2)
            .on_press(move |s: &mut App| s.ui.settings.delete_project = Some(remove.clone()))
            .build(t),
    );
    page.child(c::section("Checkouts", vec![checkout], t))
        .child(c::section("Danger", vec![danger], t))
}

/// Web `ProjectDefaultsSettings` with category "project": the picked project's own model and
/// workspace for new threads, read from and written to its `projectSettingsOverrides` entry.
/// The fallback model is environment-wide, so it stays on Threads.
fn new_threads(s: &App, cx: &rune::Cx<App>, t: &T) -> El {
    let rows = [
        json!({"key": "defaultModelSelection", "title": "Model", "kind": "model", "options": [],
            "description": "Model for new threads in this project."}),
        json!({"key": "defaultThreadEnvMode", "title": "Workspace", "kind": "select",
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
    let mut rows = vec![super::about::about_row(
        t,
        "Actions",
        "Commands that run in this project's checkout or its worktree, with optional shortcuts.",
        crate::kit::Button::new("Add action")
            .variant("outline")
            .size("xs")
            .icon(&crate::icons::PLUS)
            .on_press(move |s: &mut App| scripts::open_for_project(s, &add, None))
            .build(t)
            .key("project-add-action"),
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

pub fn confirm_remove(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, title)) = &s.ui.settings.delete_project else {
        return;
    };
    let project = s.state().projects.iter().find(|p| p["id"] == id.as_str());
    let root = project.map(|p| crate::data::s(p, "workspaceRoot")).unwrap_or("");
    let environment = s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str());
    let threads = s.state().threads.iter().filter(|thread| thread["projectId"] == id.as_str() && listed(thread)).count();
    let (question, details) = removal_message(title, root, environment, threads);
    let id = id.clone();
    crate::kit::Dialog::new("remove-project", question)
        .open(true)
        .alert()
        .description(details)
        .on_close(|s: &mut App| s.ui.settings.delete_project = None)
        .child(
            row()
                .gap(8.0)
                .child(
                    crate::kit::Button::new("Cancel")
                        .variant("outline")
                        .on_press(|s: &mut App| s.ui.settings.delete_project = None)
                        .build(t),
                )
                .child(
                    crate::kit::Button::new("Remove project")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            request(
                                s,
                                "projects.mutate",
                                json!({"type":"project.delete","commandId":s5_net::uuid(),"projectId":id,"force":true}),
                            );
                            s.ui.settings.delete_project = None;
                            s.ui.settings.project = None;
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
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
        s.ui.settings.error = Some("Project name cannot be empty.".into());
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
fn archive_groups(projects: &[Value], threads: &[Value]) -> Vec<(Value, Vec<Value>)> {
    let key = |t: &Value| t["archivedAt"].as_str().or(t["createdAt"].as_str()).unwrap_or("").to_owned();
    let mut seen = std::collections::HashSet::new();
    projects
        .iter()
        .filter(|p| seen.insert(crate::data::s(p, "id").to_owned()))
        .filter_map(|project| {
            let mut rows: Vec<Value> = threads.iter().filter(|t| t["projectId"] == project["id"]).cloned().collect();
            rows.sort_by(|a, b| key(b).cmp(&key(a)).then_with(|| crate::data::s(b, "id").cmp(crate::data::s(a, "id"))));
            (!rows.is_empty()).then(|| (project.clone(), rows))
        })
        .collect()
}

pub fn archived(s: &App, t: &T) -> El {
    let result = s.ui.settings.results.get(ARCHIVED_SNAPSHOT);
    let fetched = result.and_then(|r| r["threads"].as_array()).map(Vec::as_slice).unwrap_or_default();
    // The archive snapshot names the projects; live ones cover threads archived since it was fetched.
    let projects: Vec<Value> = result
        .and_then(|r| r["projects"].as_array())
        .into_iter()
        .flatten()
        .chain(&s.state().projects)
        .filter(|p| s.ui.settings.project.as_ref().is_none_or(|id| p["id"] == *id))
        .cloned()
        .collect();
    let threads = archived_rows(fetched, &s.state().archived, &s.state().threads);
    let groups = archive_groups(&projects, &threads);
    if groups.is_empty() {
        let loading = result.is_none() && s.ui.settings.pending.values().any(|m| m == ARCHIVED_SNAPSHOT);
        let (glyph, title, description) = if loading {
            (crate::kit::Spinner::new().size("sm").tone("muted").build(&crate::kit::theme(t)), "Loading archived threads", "Checking connected environments.")
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
        let rows = threads.iter().map(|thread| archived_row(s, t, thread)).collect();
        page = page.child(col().w_full().gap(10.0).child(header).child(c::card(rows, t)));
    }
    page
}

/// Web's archived `SettingsRow`: when it was archived and created, Unarchive, and a right-click
/// menu.
fn archived_row(s: &App, t: &T, thread: &Value) -> El {
    let id = crate::data::s(thread, "id").to_owned();
    let title = crate::data::s(thread, "title").to_owned();
    let created = crate::data::s(thread, "createdAt");
    let archived_at = thread["archivedAt"].as_str().unwrap_or(created);
    let description = format!(
        "Archived {} \u{b7} Created {}",
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
    dispatch(s, json!({"type": "thread.unarchive", "threadId": id}));
}

/// Web `useThreadActions.confirmAndDeleteThread`: asks first while `confirmThreadDelete` is on.
fn request_delete(s: &mut App, id: String, title: String) {
    let key = "confirmThreadDelete";
    let confirm = s.client_settings.values.get(key).or_else(|| super::client::defaults().get(key)).and_then(Value::as_bool).unwrap_or(false);
    if confirm {
        s.ui.settings.delete_thread = Some((id, title));
    } else {
        dispatch(s, json!({"type": "thread.delete", "threadId": id}));
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
fn dispatch(s: &mut App, command: Value) {
    if let Some(net) = s.src.net() {
        let id = net.dispatch(command);
        s.ui.settings.pending.insert(id, "thread action".into());
    }
}
pub fn confirm_delete(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((id, title)) = &s.ui.settings.delete_thread else {
        return;
    };
    let id = id.clone();
    // Web's destructive `dialogs.confirm` from `confirmAndDeleteThread`.
    crate::kit::Dialog::new("delete-thread", format!("Delete thread \"{title}\"?"))
        .open(true)
        .alert()
        .width(448.0)
        .description("This permanently clears conversation history for this thread.")
        .on_close(|s: &mut App| s.ui.settings.delete_thread = None)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(
                    crate::kit::Button::new("Cancel")
                        .variant("outline")
                        .on_press(|s: &mut App| s.ui.settings.delete_thread = None)
                        .build(t),
                )
                .child(
                    crate::kit::Button::new("Delete")
                        .variant("destructive")
                        .on_press(move |s: &mut App| {
                            dispatch(s, json!({"type":"thread.delete","threadId":id}));
                            s.ui.settings.delete_thread = None;
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
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
    let projects = s.state().projects.clone();
    s.ui.settings.results.insert(ARCHIVED_SNAPSHOT.into(), json!({"projects": projects, "threads": threads}));
}

#[cfg(test)]
mod tests {
    use super::{archive_groups, archived_rows, removal_message, request_delete};
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
        let groups = archive_groups(&projects, &threads);
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
        assert_eq!(s.ui.settings.delete_thread, Some(("t".into(), "Fix it".into())), "on by default");
        s.ui.settings.delete_thread = None;
        s.client_settings.values.insert("confirmThreadDelete".into(), json!(false));
        request_delete(&mut s, "t".into(), "Fix it".into());
        assert_eq!(s.ui.settings.delete_thread, None);
    }
}
