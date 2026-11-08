//! The Source control page's server scan (web `SourceControlSettingsPanel`): version control
//! tools and hosting integrations found on the server. The Repositories and Writing & review
//! sections around it are ordinary `pages.json` rows.

use super::{controls as c, request};
use crate::{
    App, icons,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

pub const DISCOVERY: &str = "server.discoverSourceControl";

pub fn scan(s: &mut App) {
    request(s, DISCOVERY, json!({}));
}

/// `Schema.Option` fields arrive as `{_tag, value}` or as the bare value.
fn opt(v: &Value) -> Option<&str> {
    v["value"].as_str().or(v.as_str()).filter(|v| !v.is_empty())
}

fn is_provider(item: &Value) -> bool {
    item.get("auth").is_some()
}

fn not_ready(item: &Value) -> bool {
    !is_provider(item) && item["implemented"] == false
}

fn available(item: &Value) -> bool {
    item["status"] == "available"
}

fn enabled(item: &Value) -> bool {
    if is_provider(item) {
        available(item) && item["auth"]["status"] == "authenticated"
    } else {
        available(item) && item["implemented"] == true
    }
}

/// `itemSummary`: the line under the label.
pub fn summary(item: &Value) -> String {
    let label = item["label"].as_str().unwrap_or("");
    let hint = item["installHint"].as_str().unwrap_or("");
    if not_ready(item) {
        return format!("Support for {label} is coming soon.");
    }
    if !available(item) {
        return format!("Not available on this server: {hint}");
    }
    if !is_provider(item) {
        return "Available".into();
    }
    let auth = &item["auth"];
    let detail = opt(&auth["detail"]);
    match auth["status"].as_str() {
        Some("authenticated") => {
            let mut line = "Authenticated".to_owned();
            if let Some(account) = opt(&auth["account"]) {
                line.push_str(&format!(" as {account}"));
            }
            if let Some(detail) = detail {
                line.push_str(&format!(" · {detail}"));
            }
            line
        }
        Some("unauthenticated") if item["executable"].as_str().is_none() => format!("Available. {hint}"),
        Some("unauthenticated")
            if auth["accounts"].as_array().is_some_and(|a| a.iter().any(|e| e["authenticated"] == true)) =>
        {
            detail.map(str::to_owned).unwrap_or_else(|| format!("Every {label} host is turned off."))
        }
        Some("unauthenticated") => format!(
            "{label} is not authenticated on this server. Sign in or configure credentials using the {} tool on the server host to enable change request features.",
            item["executable"].as_str().unwrap_or("")
        ),
        _ => format!("Could not verify {label}. {}", detail.unwrap_or(hint)),
    }
}

fn dot(item: &Value, t: &T) -> rune::Color {
    if not_ready(item) || (is_provider(item) && !enabled(item)) {
        t.c.muted_foreground.alpha_mul(0.35)
    } else if !available(item) {
        t.c.warning
    } else {
        t.c.success
    }
}

fn item_row(item: &Value, t: &T) -> El {
    let mut heading = row()
        .gap(8.0)
        .center_y()
        .child(el().size(8.0, 8.0).radius(4.0).fill(dot(item, t)))
        .child(text(item["label"].as_str().unwrap_or("").to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground));
    if let Some(version) = opt(&item["version"]) {
        heading = heading.child(text(version.to_owned()).font(theme::MONO_XS).color(t.c.muted_foreground));
    }
    if not_ready(item) {
        heading = heading.child(crate::kit::badge("Coming Soon", "warning", "sm", t));
    }
    if is_provider(item) && item["auth"]["status"] == "unauthenticated" {
        heading = heading.child(crate::kit::badge("Not authenticated", "secondary", "sm", t));
    }
    let mut line = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(heading)
                .child(text(summary(item)).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground.alpha_mul(0.8)).wrap()),
        );
    if !not_ready(item) {
        line = line.child(
            crate::kit::Switch::new(enabled(item))
                .label(format!("{} availability", item["label"].as_str().unwrap_or("")))
                .hide_label()
                .disabled(true)
                .build(&crate::kit::theme(t)),
        );
    }
    if not_ready(item) { line.opacity(0.8) } else { line }
}

fn rescan(pending: bool, t: &T) -> El {
    crate::kit::Button::new("Rescan server environment")
        .variant("ghost-muted")
        .size("icon-xs")
        .icon(&icons::REFRESH_CW)
        .disabled(pending)
        .on_press(scan)
        .build(t)
}

/// The scan sections, in web order after Repositories.
pub fn view(s: &App, t: &T) -> Vec<El> {
    let pending = s.ui.settings.pending.values().any(|m| m == DISCOVERY);
    let Some(result) = s.ui.settings.results.get(DISCOVERY) else {
        let body = text(if pending { "Scanning the server environment…" } else { "Connect an environment to inspect its version control tools and hosting integrations." })
            .font(theme::TEXT_SM)
            .color(t.c.muted_foreground)
            .px(16.0)
            .py(12.0);
        return vec![c::section_with_actions("Version Control", vec![rescan(pending, t)], vec![body], t)];
    };
    let rows = |key: &str| -> Vec<El> {
        result[key].as_array().into_iter().flatten().map(|item| item_row(item, t)).collect()
    };
    let (vcs, hosts) = (rows("versionControlSystems"), rows("sourceControlProviders"));
    let mut out = Vec::new();
    let has_vcs = !vcs.is_empty();
    if has_vcs {
        out.push(c::section_with_actions("Version Control", vec![rescan(pending, t)], vcs, t));
    }
    if !hosts.is_empty() {
        let actions = if has_vcs { vec![] } else { vec![rescan(pending, t)] };
        out.push(c::section_with_actions("Source control providers", actions, hosts, t));
    }
    if out.is_empty() {
        let empty = col()
            .w_full()
            .padding(24.0)
            .gap(8.0)
            .items_center()
            .child(text("Nothing detected yet").font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(
                text("Install Git on the server, add optional hosting integrations or credentials your workspace needs, then rescan.")
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .wrap(),
            )
            .child(
                crate::kit::Button::new("Scan")
                    .variant("outline")
                    .size("compact")
                    .disabled(pending)
                    .on_press(scan)
                    .build(t),
            );
        out.push(c::section("Server environment", vec![empty], t));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn summaries_follow_web_item_summary() {
        let git = json!({"kind":"git","label":"Git","implemented":true,"status":"available","installHint":"Install Git"});
        assert_eq!(summary(&git), "Available");
        let jj = json!({"kind":"jj","label":"Jujutsu","implemented":false,"status":"missing","installHint":"x"});
        assert_eq!(summary(&jj), "Support for Jujutsu is coming soon.");
        let gh = json!({"kind":"github","label":"GitHub","executable":"gh","status":"available","installHint":"Install gh",
            "auth":{"status":"authenticated","account":{"_tag":"Some","value":"octo"},"detail":{"_tag":"None"}}});
        assert_eq!(summary(&gh), "Authenticated as octo");
        let signed_out = json!({"kind":"github","label":"GitHub","executable":"gh","status":"available","installHint":"Install gh",
            "auth":{"status":"unauthenticated","account":null,"detail":null}});
        assert!(summary(&signed_out).starts_with("GitHub is not authenticated on this server."));
        let missing = json!({"kind":"gitlab","label":"GitLab","status":"missing","installHint":"Install glab","auth":{"status":"unknown"}});
        assert_eq!(summary(&missing), "Not available on this server: Install glab");
    }
}
