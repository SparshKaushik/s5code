use crate::{
    App, data, icons, kit,
    theme::{self, T},
};
use rune::{El, ScrollSpec, col, el, icon, row, text};
use serde_json::{Value, json};
#[derive(Default)]
pub struct Ui {
    entries: Vec<Value>,
    directory: String,
    selected: String,
    contents: String,
    pending: Option<(u64, bool)>,
    error: Option<String>,
}
pub fn load(app: &mut App, directory: String) {
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    let request = app.src.net().map(|n| {
        n.request(
            "projects.listEntries",
            json!({"cwd":cwd,"directoryPath":directory}),
        )
    });
    app.ui.panel.files.directory = directory;
    app.ui.panel.files.pending = request.map(|id| (id, false));
    app.ui.panel.files.error = None;
}
pub(super) fn read(app: &mut App, path: String) {
    let Some((_, cwd)) = super::context(app) else {
        return;
    };
    let request = app
        .src
        .net()
        .map(|n| n.request("projects.readFile", json!({"cwd":cwd,"relativePath":path})));
    app.ui.panel.files.selected = path;
    app.ui.panel.files.pending = request.map(|id| (id, true));
    app.ui.panel.files.error = None;
}
pub fn poll(app: &mut App) -> bool {
    let Some((id, file)) = app.ui.panel.files.pending else {
        return false;
    };
    let Some(reply) = app.src.net().and_then(|n| n.take_reply(id)) else {
        return false;
    };
    let ui = &mut app.ui.panel.files;
    ui.pending = None;
    match reply {
        Ok(v) => {
            if file {
                ui.contents = data::s(&v, "contents").into()
            } else {
                ui.entries = v["entries"].as_array().cloned().unwrap_or_default()
            }
        }
        Err(e) => ui.error = Some(e.to_string()),
    };
    true
}
pub fn view(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.files;
    let header = row()
        .h(40.)
        .px(8.)
        .gap(6.)
        .center_y()
        .child(
            kit::Button::new("Parent folder")
                .icon(&icons::ARROW_UP)
                .variant("ghost-muted")
                .size("icon-xs")
                .on_press(|s: &mut App| {
                    let path =
                        s.ui.panel
                            .files
                            .directory
                            .rsplit_once('/')
                            .map(|(p, _)| p.to_string())
                            .unwrap_or_default();
                    s.ui.panel.files.selected.clear();
                    load(s, path);
                })
                .build(t),
        )
        .child(
            text(if ui.selected.is_empty() {
                if ui.directory.is_empty() {
                    "Files"
                } else {
                    &ui.directory
                }
            } else {
                &ui.selected
            })
            .font(theme::TEXT_XS)
            .color(t.c.contrast_foreground)
            .grow(1.),
        )
        .child(
            kit::Button::new("Refresh files")
                .icon(&icons::REFRESH_CW)
                .variant("ghost-muted")
                .size("icon-xs")
                .on_press(|s: &mut App| load(s, s.ui.panel.files.directory.clone()))
                .build(t),
        );
    let content = if let Some(error) = &ui.error {
        super::notice(t, error)
    } else if !ui.selected.is_empty() {
        rune_kit::markdown::Markdown::new(format!(
            "```{}\n{}\n```",
            ui.selected.rsplit('.').next().unwrap_or(""),
            ui.contents
        ))
        .render(
            &rune_kit::markdown::MarkdownStyle::neutral(t.dark)
                .code_block(t.c.background, rune::Color::TRANSPARENT)
                .code_block_font(theme::mono(theme::TEXT_XS).lh(20.)),
        )
    } else {
        let mut list = col().w_full().padding(8.).gap(2.);
        for entry in &ui.entries {
            let path = data::s(entry, "path").to_string();
            let directory = entry["kind"] == "directory";
            list = list.child(
                row()
                    .w_full()
                    .h(28.)
                    .px(8.)
                    .gap(8.)
                    .center_y()
                    .radius(4.)
                    .hover(t.c.accent)
                    .focusable(true)
                    .role("button")
                    .label(&path)
                    .child(
                        icon(
                            if directory {
                                &icons::FOLDER
                            } else {
                                &icons::FILE
                            },
                            14.,
                        )
                        .size(14., 14.)
                        .color(t.c.muted_foreground),
                    )
                    .child(
                        text(path.rsplit('/').next().unwrap_or(&path))
                            .font(theme::TEXT_XS)
                            .color(t.c.contrast_foreground),
                    )
                    .on_press(move |s: &mut App| {
                        if directory {
                            load(s, path.clone())
                        } else {
                            read(s, path.clone())
                        }
                    }),
            );
        }
        list
    };
    col()
        .grow(1.)
        .min_h(0.)
        .child(header)
        .child(el().h(1.).w_full().fill(t.c.border))
        .child(
            col()
                .grow(1.)
                .min_h(0.)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(content),
        )
}
pub fn fixture(app: &mut App) {
    let v: Value = serde_json::from_str(include_str!("../../fixtures/panel/files.json")).unwrap();
    app.ui.panel.files.entries = v["entries"].as_array().unwrap().clone();
}
