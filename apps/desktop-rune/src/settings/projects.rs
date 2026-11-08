use super::{controls as c, request};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::{Value, json};

pub fn view(s: &App, t: &T) -> El {
    let selected =
        s.ui.settings
            .project
            .as_ref()
            .and_then(|id| s.state().projects.iter().find(|p| p["id"] == *id));
    let Some(project) = selected else {
        return text("Choose a project to manage its name, icon, checkouts and actions.")
            .px(16.0)
            .font(theme::TEXT_SM)
            .color(t.c.muted_foreground);
    };
    let id = crate::data::s(project, "id");
    let mut fields = col().w_full().gap(16.0).padding(16.0);
    for (field, label) in [("title", "Name"), ("faviconPath", "Icon path")] {
        let key = format!("project::{id}::{field}");
        let (owned, submit) = (key.clone(), key.clone());
        let edit =
            s.ui.settings
                .edits
                .get(&key)
                .cloned()
                .unwrap_or_else(|| rune::TextEdit::new(project[field].as_str().unwrap_or("")));
        fields = fields.child(
            col()
                .w_full()
                .gap(8.0)
                .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    crate::kit::Input::new(&edit)
                        .placeholder(label)
                        .on_edit(move |s: &mut App| {
                            s.ui.settings
                                .edits
                                .entry(owned.clone())
                                .or_insert_with(|| edit.clone())
                        })
                        .on_submit(move |s: &mut App, _: &mut rune::Tx| {
                            if let Some(text) = s.ui.settings.edits.get(&submit).map(|e| e.text.clone()) {
                                super::commit_text(s, &submit, &text);
                            }
                        })
                        .build(t)
                        .key(key)
                        .single_line(),
                ),
        );
    }
    fields = fields.child(
        text(crate::data::s(project, "workspaceRoot").to_owned())
            .font(theme::MONO_XS)
            .color(t.c.muted_foreground),
    );
    c::section("Project", vec![fields], t)
}
pub fn value(s: &App, id: &str, field: &str) -> Value {
    s.state()
        .projects
        .iter()
        .find(|p| p["id"] == id)
        .map(|p| p[field].clone())
        .unwrap_or(Value::Null)
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
    use super::archived_rows;
    use serde_json::json;

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
