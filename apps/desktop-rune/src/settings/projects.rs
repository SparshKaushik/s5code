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

    let icon = match &project["projectIcon"] {
        icon if icon["kind"] == "lucide" => format!("{} · {}", crate::data::s(icon, "name"), crate::data::s(icon, "color")),
        icon if icon["kind"] == "monogram" => format!("{} · {}", crate::data::s(icon, "text"), crate::data::s(icon, "color")),
        icon if icon["kind"] == "emoji" => crate::data::s(icon, "emoji").to_owned(),
        _ => project["faviconPath"].as_str().unwrap_or("Automatic").to_owned(),
    };
    let mut overview = vec![
        c::setting_row(
            s,
            cx,
            t,
            &json!({"key": key("title"), "title": "Name", "kind": "input", "width": 256, "placeholder": "Project name",
                "description": "The shared name for this project group in the sidebar and thread lists."}),
        ),
        c::setting_row(
            s,
            cx,
            t,
            &json!({"key": key("faviconPath"), "title": "Project icon", "kind": "input", "width": 256,
                "placeholder": "Automatic", "description": icon}),
        ),
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
        .child(actions(s, t));

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

/// Subagents and side chats live inside their parent's view (`isListedThreadLineage`).
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
                .child(crate::shell::sidebar::monogram(t, &title, false))
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

fn listed(thread: &Value) -> bool {
    thread["lineage"]["relationshipToParent"] != "subagent" && thread["lineage"]["sideChat"] != true
}

/// Web `ProjectActionsSettings`' list. Adding and editing actions needs the script editor
/// dialog, which the native client doesn't have yet.
fn actions(s: &App, t: &T) -> El {
    let scripts = super::value(s, "defaultProjectScripts");
    let mut rows = vec![super::about::about_row(
        t,
        "Actions",
        "Commands that run in this project's checkout or its worktree, with optional shortcuts.",
        el(),
    )];
    let scripts = scripts.as_array().map(Vec::as_slice).unwrap_or_default();
    if scripts.is_empty() {
        rows.push(
            text("No actions configured.")
                .font(theme::TEXT_SM)
                .color(t.c.muted_foreground)
                .px(16.0)
                .py(10.0),
        );
    }
    for script in scripts {
        let mut heading = row()
            .gap(8.0)
            .center_y()
            .child(rune::icon(&crate::icons::PLAY, 16.0).color(t.c.muted_foreground))
            .child(text(crate::data::s(script, "name").to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate());
        for (on, label) in [
            (script["runOnWorktreeCreate"] == true, "setup"),
            (script["runOnSettle"] == true, "on settle"),
            (script["previewUrl"].is_string(), "preview · desktop only"),
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
        rows.push(
            col()
                .w_full()
                .px(16.0)
                .py(8.0)
                .gap(4.0)
                .child(heading)
                .child(text(crate::data::s(script, "command").to_owned()).font(theme::MONO_XS).color(t.c.muted_foreground).truncate()),
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

pub fn archived(s: &App, t: &T) -> El {
    let fetched = s.ui.settings.results.get(ARCHIVED_SNAPSHOT).and_then(|r| r["threads"].as_array());
    let fetched = fetched.map(Vec::as_slice).unwrap_or_default();
    let threads: Vec<_> = archived_rows(fetched, &s.state().archived, &s.state().threads)
        .into_iter()
        .filter(|thread| {
            s.ui.settings
                .project
                .as_ref()
                .is_none_or(|id| thread["projectId"] == *id)
        })
        .collect();
    if threads.is_empty() {
        return c::section(
            "Archive",
            vec![
                col()
                    .padding(16.0)
                    .gap(4.0)
                    .child(
                        text("No archived threads")
                            .font(theme::SM_MEDIUM)
                            .color(t.c.foreground),
                    )
                    .child(
                        text("Archived threads will appear here.")
                            .font(theme::TEXT_XS)
                            .color(t.c.muted_foreground),
                    ),
            ],
            t,
        );
    }
    let mut rows = Vec::new();
    for thread in &threads {
        let restore = crate::data::s(thread, "id").to_owned();
        let delete = restore.clone();
        let title = crate::data::s(thread, "title").to_owned();
        rows.push(
            row()
                .w_full()
                .padding(16.0)
                .gap(8.0)
                .center_y()
                .child(
                    text(title.clone())
                        .font(theme::TEXT_SM)
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .child(c::button("Restore", t).on_press(move |s: &mut App| {
                    dispatch(s, json!({"type":"thread.unarchive","threadId":restore}))
                }))
                .child(
                    crate::kit::Button::new("Delete")
                        .variant("ghost-destructive")
                        .size("sm")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.delete_thread = Some((delete.clone(), title.clone()))
                        })
                        .build(t),
                ),
        );
    }
    c::section("Archived threads", rows, t)
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
    crate::kit::Dialog::new("delete-thread", "Delete thread?")
        .open(true)
        .alert()
        .description(format!("Permanently delete {title} and its chat history?"))
        .on_close(|s: &mut App| s.ui.settings.delete_thread = None)
        .child(
            row()
                .gap(8.0)
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

#[cfg(test)]
mod tests {
    use super::{archived_rows, removal_message};
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
}
