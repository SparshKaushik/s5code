use super::controls as c;
use crate::App;
use crate::theme::{self, AlphaMul, T};
use rune::{Cx, El, col, row, text};
use serde_json::Value;
use std::sync::OnceLock;

pub fn specs() -> &'static Value {
    static PAGES: OnceLock<Value> = OnceLock::new();
    PAGES.get_or_init(|| {
        let modifier = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
        serde_json::from_str(&include_str!("pages.json").replace("{mod}", modifier)).unwrap()
    })
}

/// Provider sign-in status fails per instance (e.g. a CLI that isn't installed); like web,
/// the auth section reports it inline instead of hiding the Providers page.
fn blocks_page(method: &str) -> bool {
    method != "provider.auth.subscribe" && method != "server.getStorageCleanupReport"
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T, path: &str) -> El {
    let mut page = col()
        .w_full()
        // Web's `width="expanded"` (max-w-6xl) for the wide diagnostics tables.
        .max_w(if path == "diagnostics" { 1152.0 } else { 896.0 })
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
        let (title, detail) = ("Something went wrong", error.as_str());
        page = page.child(
            row()
                .w_full()
                .px(16.0)
                .py(12.0)
                .gap(10.0)
                .radius(12.0)
                .border(1.0, t.c.destructive.alpha_mul(0.32))
                .fill(t.c.destructive.alpha_mul(0.06))
                .child(rune::icon(&crate::icons::CIRCLE_ALERT, 16.0).color(t.c.destructive).mt(2.0))
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(detail.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap()),
                ),
        );
    }
    if s.ui
        .settings
        .active_resources
        .iter()
        .any(|(method, payload)| {
            blocks_page(method)
                && super::resource(s, method, payload.clone()).is_some_and(|v| v.get("error").is_some())
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
            // Snooze options and the fallback model are environment-wide; project scope hides them.
            let project = s.ui.settings.project.is_some();
            if project && title == "Snooze" || !c::supported(s.state().config.as_ref(), section) {
                continue;
            }
            let rows = section["rows"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|spec| !(project && spec["key"] == "fallbackModelSelection"))
                .filter(|spec| c::visible(s, spec))
                .filter(|spec| spec["macOnly"] != true || cfg!(target_os = "macos"))
                .map(|spec| c::setting_row(s, cx, t, spec));
            // `AdvancedSettings`: a fold at the end of the previous section's card.
            if title == "Advanced" {
                if let Some((section, last)) = groups.last_mut() {
                    // Each section's fold opens on its own, as web's per-section `AdvancedSettings`.
                    let fold = format!("{path}/{section}");
                    let open = s.ui.settings.advanced.contains(&fold);
                    last.push(advanced_trigger(fold, open, t));
                    if open {
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
            // Web's server scan sits between Repositories and Writing & review.
            if path == "source-control" && title == "Repositories" {
                for section in super::source_control::view(s, t) {
                    page = page.child(section);
                }
            }
            if path == "maintenance" && title == "Updates" {
                page = page.child(super::background::view(s, cx, t));
                for section in super::storage::maintenance(s, cx, t) {
                    page = page.child(section);
                }
                if !super::storage::project_scope(s) {
                    page = page.child(diagnostics_link(s, t));
                }
            }
            if path == "worktrees" && title == "Branches" {
                page = page.child(super::storage::worktree_cleanup(s, cx, t));
            }
        }
        return page;
    }
    page.child(match path {
        "providers" => super::providers::view(s, cx, t),
        "devices" => super::devices::view(s, t),
        "connections" => super::connections::view(s, t),
        "access" => super::connections::access(s, t),
        "scheduled-tasks" => super::tasks::view(s, t),
        "keybindings" => super::keybindings::view(s, t),
        "projects" => super::projects::view(s, cx, t),
        "archived" => super::projects::archived(s, t),
        "about" => super::about::view(s, t),
        "snap-shot" => super::snapshots::view(s, t),
        "secrets" => super::secrets::view(s, t),
        "diagnostics" => super::diagnostics::view(s, cx, t),
        super::licenses::PATH => super::licenses::view(s, t),
        _ => unavailable(path, t),
    })
}

/// Web Maintenance's Diagnostics section: the way into the detail page.
fn diagnostics_link(s: &App, t: &T) -> El {
    let description = if s.ui.settings.environment_only {
        "Inspect processes, resource use, and logs on this environment."
    } else {
        "Inspect processes, resource use, and logs on one environment at a time."
    };
    c::section(
        "Diagnostics",
        vec![super::about::about_row(
            t,
            "Diagnostics",
            description,
            crate::kit::Button::new("View diagnostics")
                .variant("outline")
                .size("sm")
                .build(t)
                .on_press(|s: &mut App| s.navigate(crate::Route::Settings("diagnostics".into()))),
        )],
        t,
    )
}

pub(super) fn advanced_trigger(path: String, open: bool, t: &T) -> El {
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
        .on_press(move |s: &mut App| {
            if !s.ui.settings.advanced.remove(&path) {
                s.ui.settings.advanced.insert(path.clone());
            }
        })
}

fn unavailable(path: &str, t: &T) -> El {
    text(format!(
        "{} settings are not yet available in the native client.",
        super::label(path)
    ))
    .font(theme::TEXT_SM)
    .color(t.c.muted_foreground)
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_auth_errors_do_not_hide_the_page() {
        assert!(!super::blocks_page("provider.auth.subscribe"));
        assert!(super::blocks_page("scheduledTasks.subscribe"));
    }
}
