//! The provider header's version advisory (web `versionAdvisoryNode`): a popover with what the
//! advisory says, Update now or Install for a recommended version, and the manual command.
use super::request;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

#[derive(Debug, PartialEq)]
pub struct Advisory {
    pub title: &'static str,
    detail: String,
    command: Option<String>,
    strong: bool,
    target: Option<String>,
}

/// `getProviderVersionLabel`.
pub fn version_label(version: &str) -> String {
    let date = version.strip_prefix("agy_acp_server_").and_then(|rest| {
        let (stamp, tail) = rest.split_once('_')?;
        (stamp.len() == 8 && stamp.chars().all(|c| c.is_ascii_digit())).then_some(())?;
        let candidate = tail.split_once('_').map(|(_, c)| c);
        Some(format!("{}-{}-{}{}", &stamp[..4], &stamp[4..6], &stamp[6..], candidate.map(|c| format!(" {c}")).unwrap_or_default()))
    });
    date.unwrap_or_else(|| if version.starts_with(|c: char| c.is_ascii_digit()) { format!("v{version}") } else { version.to_owned() })
}

fn some_str(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// `getProviderVersionAdvisoryPresentation`.
pub fn advisory(live: &Value, enabled: bool) -> Option<Advisory> {
    let version = &live["versionAdvisory"];
    let compatibility = &live["compatibilityAdvisory"];
    let latest_bad = matches!(compatibility["latestVersionStatus"].as_str(), Some("broken" | "unsupported"));
    let status = compatibility["status"].as_str();
    if enabled && matches!(status, Some("graceful" | "unsupported" | "broken")) {
        let target = some_str(&compatibility["recommendedVersion"]);
        let recommendation = target.as_deref().map(version_label).or_else(|| some_str(&compatibility["recommendedRange"]));
        let current = version["status"] == "current";
        return Some(Advisory {
            title: match status {
                Some("graceful") => "Limited support",
                Some("unsupported") => "Unsupported version",
                _ => "Known broken version",
            },
            detail: some_str(&compatibility["message"]).unwrap_or_else(|| match recommendation {
                Some(r) => format!("Use {r} for full support."),
                None => "Update for full support.".into(),
            }),
            command: if target.is_some() || latest_bad || current { None } else { some_str(&version["updateCommand"]) },
            strong: status != Some("graceful"),
            target,
        });
    }
    if !version.is_object() || matches!(version["status"].as_str(), Some("current" | "unknown")) || latest_bad {
        return None;
    }
    let label = "Update available";
    Some(Advisory {
        title: label,
        detail: some_str(&version["message"]).unwrap_or_else(|| match some_str(&version["latestVersion"]) {
            Some(v) => format!("{label}: install {}.", version_label(&v)),
            None => format!("{label}: install the latest provider version."),
        }),
        command: some_str(&version["updateCommand"]),
        strong: false,
        target: None,
    })
}

fn updating(live: &Value) -> bool {
    matches!(live["updateState"]["status"].as_str(), Some("queued" | "running"))
}

/// What the action runs, if anything: `onInstallRecommended` when the advisory recommends a
/// version this provider can install, else `onRunUpdate` for a one-click update candidate.
fn action(live: &Value, enabled: bool, a: &Advisory) -> Option<Option<String>> {
    let version = &live["versionAdvisory"];
    if let Some(target) = &a.target {
        let installable = some_str(&live["compatibilityAdvisory"]["message"]).is_some() && version["canInstallVersion"] == true;
        return installable.then(|| Some(target.clone()));
    }
    let latest_bad = matches!(live["compatibilityAdvisory"]["latestVersionStatus"].as_str(), Some("broken" | "unsupported"));
    let candidate = enabled
        && live["enabled"] != false
        && !latest_bad
        && version["status"] == "behind_latest"
        && version["canUpdate"] == true
        && some_str(&version["updateCommand"]).is_some();
    candidate.then_some(None)
}

/// An advisory trigger (the list row's or the header's), mounted under its popover's anchor key.
pub fn trigger(s: &App, t: &T, id: &str, live: Option<&Value>, enabled: bool, place: &'static str) -> Option<El> {
    let live = live?;
    let a = advisory(live, enabled)?;
    let busy = updating(live);
    let warn = live["compatibilityAdvisory"]["status"].as_str().is_some_and(|s| matches!(s, "graceful" | "unsupported" | "broken")) && enabled;
    let glyph = if busy {
        &icons::LOADER_CIRCLE
    } else if warn {
        &icons::TRIANGLE_ALERT
    } else {
        &icons::CIRCLE_ARROW_UP
    };
    let this = (id.to_owned(), place);
    let open = s.ui.settings.provider_advisory.as_ref() == Some(&this);
    let button = kit::Button::new(format!("{}, view details", if busy { "Updating" } else { a.title }))
        .variant("ghost-muted")
        .size(if place == "row" { "icon-micro" } else { "icon-xs" })
        .on_press(move |s: &mut App| {
            let current = &mut s.ui.settings.provider_advisory;
            *current = if current.as_ref() == Some(&this) { None } else { Some(this.clone()) };
        })
        .build(t)
        .expanded(open)
        .child(rune::icon(glyph, 14.0).color(if warn { t.c.warning } else { t.c.muted_foreground }));
    Some(row().key(format!("advisory-{place}-{id}-trigger")).child(button))
}

fn panel(t: &T, id: &str, live: &Value, enabled: bool, name: &str, a: &Advisory) -> El {
    let busy = updating(live);
    let mut body = col().w(320.0).gap(12.0).padding(16.0).child(
        col()
            .w_full()
            .gap(2.0)
            .child(text(a.title).font(theme::weight(theme::TEXT_SM, 600.0)).color(t.c.foreground))
            .child(text(a.detail.clone()).font(theme::TEXT_XS.lh(16.0)).color(if a.strong { t.c.warning } else { t.c.muted_foreground }).wrap()),
    );
    let run = action(live, enabled, a);
    if let Some(target) = run.clone() {
        let label = if busy {
            "Updating".to_owned()
        } else {
            target.as_deref().map_or("Update now".to_owned(), |v| format!("Install {}", version_label(v)))
        };
        let (id, driver) = (id.to_owned(), live["driver"].as_str().unwrap_or("").to_owned());
        body = body.child(
            kit::Button::new(label)
                .variant("outline")
                .size("xs")
                .icon(if busy { &icons::LOADER_CIRCLE } else { &icons::DOWNLOAD })
                .disabled(busy)
                .on_press(move |s: &mut App| {
                    let mut input = json!({"provider": driver, "instanceId": id});
                    if let Some(version) = &target {
                        input["targetVersion"] = json!(version);
                    }
                    request(s, "server.updateProvider", input);
                })
                .build(t)
                .w_full(),
        );
    }
    let status = live["updateState"]["status"].as_str();
    let progress = busy.then(|| some_str(&live["updateState"]["message"]).unwrap_or_else(|| "Starting update".into()));
    let problem = matches!(status, Some("failed" | "unchanged")).then(|| some_str(&live["updateState"]["message"])).flatten();
    if let Some(line) = progress.as_ref().or(problem.as_ref()) {
        body = body.child(
            text(line.clone()).font(theme::TEXT_XS.lh(16.0)).color(if problem.is_some() { t.c.warning } else { t.c.muted_foreground }).wrap(),
        );
    }
    if let Some(command) = &a.command {
        if run.is_some() {
            let rule = || el().grow(1.0).h(1.0).fill(t.c.border);
            body = body.child(
                row()
                    .w_full()
                    .gap(8.0)
                    .center_y()
                    .child(rule())
                    .child(text("OR, UPDATE MANUALLY USING").font(theme::weight(theme::TEXT_3XS, 500.0)).color(t.c.muted_foreground))
                    .child(rule()),
            );
        }
        let (copied, toast) = (command.clone(), format!("{name} update command copied"));
        body = body.child(
            row()
                .w_full()
                .gap(4.0)
                .center_y()
                .radius(6.0)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.muted.alpha_mul(0.4))
                .py(2.0)
                .pl(8.0)
                .pr(2.0)
                .child(text(command.clone()).font(theme::mono(theme::TEXT_2XS)).color(t.c.foreground).truncate().grow(1.0).min_w(0.0))
                .child(
                    kit::Button::new("Copy update command")
                        .variant("ghost-muted")
                        .size("icon-xs")
                        .icon(&icons::COPY)
                        .build(t)
                        .on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
                            tx.copy(&copied);
                            crate::shell::palette::toast(s, &toast, Some("Run it in a terminal when you are ready to update."));
                        }),
                ),
        );
    }
    body
}

pub fn popover(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some((id, place)) = &s.ui.settings.provider_advisory else { return };
    let Some(live) = crate::chat::composer::provider(s, id) else { return };
    let instance = &super::server_settings(s)["providerInstances"][id.as_str()];
    let enabled = if instance.is_object() { super::providers::resolve_enabled(instance) } else { live["enabled"] != false };
    let Some(a) = advisory(live, enabled) else { return };
    let name = live["displayName"].as_str().unwrap_or(id.as_str()).to_owned();
    let _trigger = kit::Popover::new(format!("advisory-{place}-{id}"))
        .open(true)
        .padding(0.0)
        .gutter(6.0)
        .placement(kit::Placement::BottomEnd)
        .label(a.title)
        .child(panel(t, id, live, enabled, &name, &a))
        .on_close(|s: &mut App| s.ui.settings.provider_advisory = None)
        .build(&kit::theme(t), layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advisories_and_their_actions_follow_web() {
        assert_eq!(version_label("1.2.3"), "v1.2.3");
        assert_eq!(version_label("agy_acp_server_20260102_7_rc1"), "2026-01-02 rc1");
        assert_eq!(version_label("agy_acp_server_20260102_7"), "2026-01-02");
        assert_eq!(version_label("nightly"), "nightly");

        let behind = json!({"enabled": true, "versionAdvisory": {"status": "behind_latest", "latestVersion": "2.0.0", "canUpdate": true, "updateCommand": "npm i -g x"}});
        let a = advisory(&behind, true).unwrap();
        assert_eq!((a.title, a.detail.as_str(), a.command.as_deref()), ("Update available", "Update available: install v2.0.0.", Some("npm i -g x")));
        assert_eq!(action(&behind, true, &a), Some(None));
        assert_eq!(action(&behind, false, &a), None);
        assert!(advisory(&json!({"versionAdvisory": {"status": "current"}}), true).is_none());

        let broken = json!({
            "versionAdvisory": {"status": "behind_latest", "updateCommand": "npm i -g x", "canInstallVersion": true},
            "compatibilityAdvisory": {"status": "broken", "recommendedVersion": "1.9.0", "message": "Use 1.9"}
        });
        let a = advisory(&broken, true).unwrap();
        assert_eq!((a.title, a.strong, a.command.as_deref()), ("Known broken version", true, None));
        assert_eq!(action(&broken, true, &a), Some(Some("1.9.0".into())));
        // A disabled provider shows no compatibility warning, only the update.
        assert_eq!(advisory(&broken, false).unwrap().title, "Update available");
        let latest_bad = json!({"versionAdvisory": {"status": "behind_latest"}, "compatibilityAdvisory": {"latestVersionStatus": "broken"}});
        assert!(advisory(&latest_bad, true).is_none());
    }
}
