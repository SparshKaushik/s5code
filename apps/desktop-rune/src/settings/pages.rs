use super::controls as c;
use crate::App;
use crate::theme::{self, T};
use rune::{Cx, El, col, text};
use serde_json::Value;
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
    if s.ui
        .settings
        .active_resources
        .iter()
        .any(|(method, payload)| {
            super::resource(s, method, payload.clone()).is_some_and(|v| v.get("error").is_some())
        })
    {
        return page
            .child(
                text("Could not load this page. Check the connection’s permissions or reconnect.")
                    .font(theme::TEXT_SM)
                    .color(t.c.destructive),
            )
            .child(c::button("Retry", t).on_press(|s: &mut App| {
                let resources = s.ui.settings.active_resources.clone();
                if let Some(net) = s.src.net() {
                    for (method, payload) in resources {
                        net.unwatch_resource(&method, payload.clone());
                        net.watch_resource(&method, payload);
                    }
                }
            }));
    }
    if !super::scope::device_only(path) {
        page = page.child(super::scope::sentence(s, t));
    }
    if path == "appearance" {
        page = page.child(super::themes::view(s, t));
    }
    if let Some(sections) = specs()[path].as_array() {
        let mut groups: Vec<(&str, Vec<El>)> = Vec::new();
        for section in sections {
            let title = section["title"].as_str().unwrap_or("");
            let rows = section["rows"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|spec| c::setting_row(s, cx, t, spec));
            // `AdvancedSettings`: a fold at the end of the previous section's card.
            if title == "Advanced" {
                if let Some((_, last)) = groups.last_mut() {
                    last.push(advanced_trigger(s.ui.settings.advanced, t));
                    if s.ui.settings.advanced {
                        last.extend(rows);
                    }
                    continue;
                }
            }
            groups.push((title, rows.collect()));
        }
        for (title, children) in groups {
            if !children.is_empty() {
                page = page.child(c::section(title, children, t));
            }
        }
        return page;
    }
    page.child(match path {
        "providers" => super::providers::view(s, t),
        "devices" => super::devices::view(s, t),
        "connections" => super::connections::view(s, t),
        "access" => super::connections::access(s, t),
        "scheduled-tasks" => super::tasks::view(s, t),
        "keybindings" => super::keybindings::view(s, t),
        "projects" => super::projects::view(s, t),
        "archived" => super::projects::archived(s, t),
        "about" => super::about::view(s, t),
        _ => unavailable(path, t),
    })
}

fn advanced_trigger(open: bool, t: &T) -> El {
    rune::row()
        .w_full()
        .min_h(40.0)
        .px(16.0)
        .gap(6.0)
        .center_y()
        .focusable(true)
        .role("button")
        .label("Advanced")
        .expanded(open)
        .child(
            rune::icon(if open { &crate::icons::CHEVRON_DOWN } else { &crate::icons::CHEVRON_RIGHT }, 14.0)
                .color(t.c.muted_foreground),
        )
        .child(text("Advanced").font(theme::XS_MEDIUM).color(t.c.muted_foreground))
        .on_press(|s: &mut App| s.ui.settings.advanced = !s.ui.settings.advanced)
}

fn unavailable(path: &str, t: &T) -> El {
    text(format!(
        "{} settings are not yet available in the native client.",
        super::label(path)
    ))
    .font(theme::TEXT_SM)
    .color(t.c.muted_foreground)
}
