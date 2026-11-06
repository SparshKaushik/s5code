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
        let owned = key.clone();
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
pub fn archived(s: &App, t: &T) -> El {
    let threads: Vec<_> = s
        .state()
        .archived
        .iter()
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
    for thread in threads {
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
