use super::{controls as c, request};
use crate::theme::{self, T};
use crate::{App, icons};
use rune::{Cx, El, col, el, row, text};
use serde_json::{Value, json};
use std::sync::OnceLock;

pub fn specs() -> &'static Value {
    static PAGES: OnceLock<Value> = OnceLock::new();
    PAGES.get_or_init(|| serde_json::from_str(include_str!("pages.json")).unwrap())
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, path: &str) -> El {
    let mut page = col()
        .w_full()
        .max_w(896.0)
        .px(24.0)
        .pt(24.0)
        .pb(48.0)
        .gap(32.0);
    if let Some(error) =
        s.ui.settings
            .error
            .as_ref()
            .or(s.client_settings.error.as_ref())
    {
        page = page.child(
            text(error.clone())
                .font(theme::TEXT_SM)
                .color(t.c.destructive),
        );
    }
    if !super::scope::device_only(path) {
        page = page.child(super::scope::sentence(s, t));
    }
    if path == "appearance" {
        page = page.child(super::themes::view(s, t));
    }
    if let Some(sections) = specs()[path].as_array() {
        for section in sections {
            let title = section["title"].as_str().unwrap_or("");
            if title == "Advanced" {
                page = page.child(
                    c::button(
                        if s.ui.settings.advanced {
                            "⌄ Advanced"
                        } else {
                            "› Advanced"
                        },
                        t,
                    )
                    .on_press(|s: &mut App| s.ui.settings.advanced = !s.ui.settings.advanced),
                );
                if !s.ui.settings.advanced {
                    continue;
                }
            }
            let children: Vec<_> = section["rows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|spec| c::setting_row(s, cx, t, spec))
                .collect();
            if !children.is_empty() {
                page = page.child(c::section(title, children, t));
            }
        }
        return page;
    }
    page.child(match path {
        "providers" => super::providers::view(s, t),
        "keybindings" => super::keybindings::view(s, t),
        "projects" => projects(s, t),
        "archived" => archived(s, t),
        "about" => about(t),
        _ => unavailable(path, t),
    })
}

fn projects(s: &App, t: &T) -> El {
    let children = s
        .state()
        .projects
        .iter()
        .map(|project| {
            row()
                .w_full()
                .padding(16.0)
                .gap(12.0)
                .child(rune::icon(&icons::FOLDER, 16.0).color(t.c.muted_foreground))
                .child(
                    col()
                        .gap(4.0)
                        .child(
                            text(crate::data::s(project, "name").to_owned())
                                .font(theme::SM_MEDIUM)
                                .color(t.c.foreground),
                        )
                        .child(
                            text(crate::data::s(project, "workspaceRoot").to_owned())
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground),
                        ),
                )
        })
        .collect();
    c::section("Projects", children, t)
}
fn archived(s: &App, t: &T) -> El {
    let children = s
        .state()
        .archived
        .iter()
        .map(|thread| {
            row().padding(16.0).w_full().child(
                text(crate::data::s(thread, "title").to_owned())
                    .font(theme::TEXT_SM)
                    .color(t.c.foreground),
            )
        })
        .collect();
    c::section("Archived threads", children, t)
}
fn about(t: &T) -> El {
    col()
        .gap(32.0)
        .w_full()
        .child(c::section(
            "App",
            vec![
                row().padding(16.0).w_full().center_y().child(
                    col()
                        .grow(1.0)
                        .gap(4.0)
                        .child(text("S5 Code").font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(
                            text("Current version of the application.")
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground),
                        ),
                ),
            ],
            t,
        ))
        .child(c::section(
            "Legal",
            vec![
                text("Privacy policy")
                    .padding(16.0)
                    .font(theme::TEXT_SM)
                    .color(t.c.foreground),
                text("Open source licenses")
                    .padding(16.0)
                    .font(theme::TEXT_SM)
                    .color(t.c.foreground),
            ],
            t,
        ))
}
fn unavailable(path: &str, t: &T) -> El {
    text(format!(
        "{} settings are not yet available in the native client.",
        super::label(path)
    ))
    .font(theme::TEXT_SM)
    .color(t.c.muted_foreground)
}
