use super::{controls as c, request, value};
use crate::{
    App,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::{Value, json};

fn tasks(s: &App) -> Vec<Value> {
    super::resource(s, "scheduledTasks.subscribe", json!({}))
        .and_then(|v| v["tasks"].as_array())
        .cloned()
        .unwrap_or_default()
}
fn models(s: &App) -> Vec<(Value, String)> {
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["providers"].as_array())
        .into_iter()
        .flatten()
        .filter(|p| p["enabled"] != false)
        .flat_map(|p| {
            p["models"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|m| {
                    Some((
                        json!({"instanceId":p["instanceId"],"model":m["slug"].as_str()?}),
                        format!(
                            "{} · {}",
                            p["displayName"].as_str().unwrap_or(""),
                            m["name"].as_str().or(m["slug"].as_str()).unwrap_or("")
                        ),
                    ))
                })
        })
        .collect()
}
pub fn edit(s: &mut App, task: Option<Value>) {
    let default_project = s.ui.settings.project.clone().or_else(|| {
        s.state()
            .projects
            .first()?
            .get("id")?
            .as_str()
            .map(str::to_owned)
    });
    let default_model = models(s)
        .first()
        .map(|(v, _)| v.clone())
        .unwrap_or(Value::Null);
    let original=task.unwrap_or_else(||json!({"title":"","prompt":"","projectId":default_project,"modelSelection":default_model,"enabled":false,"schedule":{"type":"interval","everyMs":3600000},"workspaceStrategy":{"type":"root"},"runtimeMode":"full-access","interactionMode":"default"}));
    for field in ["title", "prompt", "projectId", "modelSelection", "enabled"] {
        s.ui.settings
            .form
            .insert(format!("form.task.{field}"), original[field].clone());
    }
    s.ui.settings.form.insert(
        "form.task.minutes".into(),
        json!(original["schedule"]["everyMs"].as_i64().unwrap_or(3600000) / 60000),
    );
    s.ui.settings.task_editor = Some(original);
}
pub fn view(s: &App, t: &T) -> El {
    let mut children = Vec::new();
    for task in tasks(s).into_iter().filter(|task| {
        s.ui.settings
            .project
            .as_ref()
            .is_none_or(|id| task["projectId"] == *id)
    }) {
        let id = task["id"].clone();
        let toggle = id.clone();
        let run = id.clone();
        let delete = id.clone();
        let original = task.clone();
        let title = task["title"].as_str().unwrap_or("Task").to_owned();
        let delete_title = title.clone();
        children.push(
            row()
                .w_full()
                .padding(16.0)
                .gap(8.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(
                            text(task["prompt"].as_str().unwrap_or("").to_owned())
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground)
                                .wrap(),
                        ),
                )
                .child(
                    c::button(
                        if task["enabled"] == true {
                            "Pause"
                        } else {
                            "Enable"
                        },
                        t,
                    )
                    .on_press(move |s: &mut App| {
                        request(
                            s,
                            "scheduledTasks.setEnabled",
                            json!({"id":toggle,"enabled":task["enabled"]!=true}),
                        )
                    }),
                )
                .child(c::button("Run now", t).on_press(move |s: &mut App| {
                    request(s, "scheduledTasks.runNow", json!({"id":run}))
                }))
                .child(
                    c::button("Edit", t)
                        .on_press(move |s: &mut App| edit(s, Some(original.clone()))),
                )
                .child(c::button("Delete", t).on_press(move |s: &mut App| {
                    s.ui.settings.delete_task = Some((delete.clone(), delete_title.clone()))
                })),
        );
    }
    if children.is_empty() {
        children.push(
            col()
                .padding(16.0)
                .gap(4.0)
                .child(
                    text("No scheduled tasks")
                        .font(theme::SM_MEDIUM)
                        .color(t.c.foreground),
                )
                .child(
                    text("Schedule agent work to run automatically.")
                        .font(theme::TEXT_XS)
                        .color(t.c.muted_foreground),
                ),
        );
    }
    col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .child(c::button("New task", t).on_press(|s: &mut App| edit(s, None))),
        )
        .child(c::section("Scheduled tasks", children, t))
}
pub fn editor(s: &App, t: &T, layers: &mut Vec<El>) {
    if let Some(task) = &s.ui.settings.task_editor {
        let mut fields = col().w_full().gap(12.0);
        for (field, label) in [("title", "Title"), ("prompt", "Prompt")] {
            let key = format!("form.task.{field}");
            let owned = key.clone();
            let edit =
                s.ui.settings
                    .edits
                    .get(&key)
                    .cloned()
                    .unwrap_or_else(|| rune::TextEdit::new(value(s, &key).as_str().unwrap_or("")));
            fields = fields
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
                        .key(key),
                );
        }
        let projects: Vec<_> = s
            .state()
            .projects
            .iter()
            .map(|p| (p["id"].clone(), crate::data::s(p, "title").to_owned()))
            .collect();
        fields = fields
            .child(text("Project").font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(c::select(
                s,
                t,
                "form.task.projectId",
                "Project",
                &projects,
                360.0,
            ))
            .child(text("Model").font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(c::select(
                s,
                t,
                "form.task.modelSelection",
                "Model",
                &models(s),
                360.0,
            ));
        if task["schedule"]["type"] == "interval" {
            let key = "form.task.minutes";
            let edit =
                s.ui.settings
                    .edits
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| rune::TextEdit::new(value(s, key).to_string()));
            fields = fields
                .child(
                    text("Repeat every (minutes)")
                        .font(theme::SM_MEDIUM)
                        .color(t.c.foreground),
                )
                .child(
                    crate::kit::Input::new(&edit)
                        .on_edit(move |s: &mut App| {
                            s.ui.settings
                                .edits
                                .entry(key.into())
                                .or_insert_with(|| edit.clone())
                        })
                        .build(t)
                        .key(key)
                        .single_line(),
                );
        }
        fields = fields
            .child(
                row()
                    .gap(8.0)
                    .center_y()
                    .child(text("Enabled").font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(c::switch(s, t, "form.task.enabled", "Enabled")),
            )
            .child(
                crate::kit::Button::new("Save task")
                    .on_press(|s: &mut App| {
                        super::commit_edits(s, true);
                        let mut payload = s.ui.settings.task_editor.as_ref().unwrap().clone();
                        for field in ["title", "prompt", "projectId", "modelSelection", "enabled"] {
                            payload[field] = value(s, &format!("form.task.{field}"));
                        }
                        if payload["title"]
                            .as_str()
                            .is_none_or(|v| v.trim().is_empty())
                            || payload["prompt"]
                                .as_str()
                                .is_none_or(|v| v.trim().is_empty())
                            || payload["projectId"].is_null()
                            || payload["modelSelection"].is_null()
                        {
                            s.ui.settings.error =
                                Some("Enter a title, prompt, project and model.".into());
                            return;
                        }
                        if payload["schedule"]["type"] == "interval" {
                            let minutes =
                                value(s, "form.task.minutes").as_u64().filter(|n| *n >= 1);
                            let Some(minutes) = minutes else {
                                s.ui.settings.error =
                                    Some("The repeat interval must be at least one minute.".into());
                                return;
                            };
                            payload["schedule"] =
                                json!({"type":"interval","everyMs":minutes*60000});
                        }
                        if !payload["id"].is_null() {
                            payload["requireExisting"] = json!(true);
                        }
                        request(s, "scheduledTasks.upsert", payload);
                    })
                    .build(t),
            );
        crate::kit::Dialog::new("task-editor", "Scheduled task")
            .open(true)
            .width(560.0)
            .on_close(|s: &mut App| s.ui.settings.task_editor = None)
            .child(fields)
            .build(t, layers);
    }
    if let Some((id, title)) = &s.ui.settings.delete_task {
        let id = id.clone();
        crate::kit::Dialog::new("delete-task", "Delete scheduled task?")
            .open(true)
            .alert()
            .description(format!("Delete {title}?"))
            .on_close(|s: &mut App| s.ui.settings.delete_task = None)
            .child(
                crate::kit::Button::new("Delete task")
                    .variant("destructive")
                    .on_press(move |s: &mut App| {
                        request(s, "scheduledTasks.delete", json!({"id":id}));
                        s.ui.settings.delete_task = None;
                    })
                    .build(t),
            )
            .build(t, layers);
    }
}
