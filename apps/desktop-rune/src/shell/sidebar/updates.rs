//! `SidebarProviderUpdatePill`: a provider update running, or how the latest one ended, above
//! the footer. Results from before this window first saw the providers stay hidden; a success
//! clears itself after three seconds, and the others until the status changes.

use std::collections::HashSet;

use rune::{Cursor, El, el, icon, row, text};
use serde_json::Value;

use crate::icons as i;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route};

const SUCCESS_VISIBLE_MS: i64 = 3_000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Loading,
    Success,
    Warning,
    Error,
}

#[derive(Clone, PartialEq, Debug)]
pub struct View {
    pub key: String,
    pub tone: Tone,
    pub title: String,
    pub description: String,
    pub dismissible: bool,
}

#[derive(Default)]
pub struct State {
    visible_after: Option<String>,
    dismissed: HashSet<String>,
    /// The success view on screen and when it appeared.
    shown: Option<(String, i64)>,
}

fn name(driver: &str) -> String {
    match driver {
        "antigravity" => "Antigravity",
        "codex" => "Codex",
        "claudeAgent" => "Claude",
        "cursor" => "Cursor",
        "grok" => "Grok",
        "fx" => "fx",
        "kiro" => "Kiro",
        "muse" => "Muse Code",
        "acpRegistry" => "ACP Registry",
        "pi" => "Pi",
        "opencode" => "OpenCode",
        other => other,
    }
    .to_owned()
}

fn list(providers: &[&Value]) -> String {
    let names: Vec<String> = providers.iter().map(|p| name(s(p, "driver"))).collect();
    match names.as_slice() {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

fn status(p: &Value) -> &str {
    s(&p["updateState"], "status")
}

fn finished(p: &Value) -> &str {
    s(&p["updateState"], "finishedAt")
}

fn version(v: &str) -> String {
    if v.starts_with('v') { v.to_owned() } else { format!("v{v}") }
}

/// One instance per driver: the default instance, else the most recently checked.
fn dedupe<'a>(providers: impl Iterator<Item = &'a Value>) -> Vec<&'a Value> {
    let mut out: Vec<&Value> = vec![];
    for p in providers {
        match out.iter_mut().find(|c| s(c, "driver") == s(p, "driver")) {
            None => out.push(p),
            Some(current) => {
                let default = s(p, "driver");
                if s(p, "instanceId") == default || (s(current, "instanceId") != default && s(p, "checkedAt") >= s(current, "checkedAt")) {
                    *current = p;
                }
            }
        }
    }
    out
}

/// Each driver's latest finished instance, so an older default-instance result cannot hide it.
fn latest_finished<'a>(providers: impl Iterator<Item = &'a Value>) -> Vec<&'a Value> {
    let mut out: Vec<&Value> = vec![];
    for p in providers {
        match out.iter_mut().find(|c| s(c, "driver") == s(p, "driver")) {
            None => out.push(p),
            Some(current) if finished(p) >= finished(current) => *current = p,
            Some(_) => {}
        }
    }
    out
}

fn key(kind: &str, providers: &[&Value]) -> String {
    let mut parts: Vec<String> = providers
        .iter()
        .map(|p| format!("{}:{}:{}", s(p, "driver"), p["updateState"]["finishedAt"].as_str().unwrap_or("pending"), s(&p["updateState"], "message")))
        .collect();
    parts.sort();
    format!("{kind}:{}", parts.join("|"))
}

/// `getProviderUpdateSidebarPillView`.
pub fn view(providers: &[Value], visible_after: Option<&str>, dismissed: &HashSet<String>) -> Option<View> {
    let active = dedupe(providers.iter().filter(|p| matches!(status(p), "queued" | "running")));
    if let Some(first) = active.first() {
        let mut parts: Vec<String> = active.iter().map(|p| format!("{}:{}", s(p, "driver"), status(p))).collect();
        parts.sort();
        let one = active.len() == 1;
        return Some(View {
            key: format!("loading:{}", parts.join("|")),
            tone: Tone::Loading,
            title: if one { format!("Updating {}", name(s(first, "driver"))) } else { format!("Updating {} providers", active.len()) },
            description: format!("{} {}", list(&active), if one { "update in progress." } else { "updates are in progress." }),
            dismissible: false,
        });
    }
    let recent = latest_finished(providers.iter().filter(|p| {
        matches!(status(p), "failed" | "unchanged" | "succeeded") && visible_after.is_none_or(|after| !finished(p).is_empty() && finished(p) >= after)
    }));
    let with = |wanted: &str| -> Vec<&Value> { recent.iter().copied().filter(|p| status(p) == wanted).collect() };
    let (failed, unchanged, succeeded) = (with("failed"), with("unchanged"), with("succeeded"));
    let mut candidates: Vec<(String, View)> = vec![];
    let latest = |group: &[&Value]| group.iter().map(|p| finished(p).to_owned()).max().unwrap_or_default();
    if let Some(first) = failed.first() {
        let provider = name(s(first, "driver"));
        let title = match (failed.len(), first["versionAdvisory"]["latestVersion"].as_str()) {
            (1, Some(v)) => format!("{provider} {} update failed", version(v)),
            (1, None) => format!("{provider} update failed"),
            (n, _) => format!("{n} provider updates failed"),
        };
        let message = s(&first["updateState"], "message");
        let description = if failed.len() == 1 && !message.is_empty() {
            message.to_owned()
        } else {
            format!("{} failed to update. Check provider settings for details.", list(&failed))
        };
        candidates.push((latest(&failed), View { key: key("failed", &failed), tone: Tone::Error, title, description, dismissible: true }));
    }
    if let Some(first) = unchanged.first() {
        let one = unchanged.len() == 1;
        candidates.push((
            latest(&unchanged),
            View {
                key: key("unchanged", &unchanged),
                tone: Tone::Warning,
                title: if one { format!("{} still needs an update", name(s(first, "driver"))) } else { format!("{} providers still need updates", unchanged.len()) },
                description: format!("{} {} outdated. Review provider settings for details.", list(&unchanged), if one { "still appears" } else { "still appear" }),
                dismissible: true,
            },
        ));
    }
    if let Some(first) = succeeded.first() {
        let one = succeeded.len() == 1;
        let provider = name(s(first, "driver"));
        let title = match (one, first["version"].as_str().filter(|v| !v.is_empty())) {
            (true, Some(v)) => format!("{provider} updated: {}", version(v)),
            (true, None) => format!("{provider} updated"),
            (false, _) => format!("{} providers updated", succeeded.len()),
        };
        let description = if one { "New sessions will use the updated provider." } else { "New sessions will use the updated providers." };
        candidates.push((latest(&succeeded), View { key: key("succeeded", &succeeded), tone: Tone::Success, title, description: description.into(), dismissible: false }));
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().map(|(_, v)| v).find(|v| !dismissed.contains(&v.key))
}

fn providers(s: &App) -> &[Value] {
    s.state().config.as_ref().and_then(|c| c["providers"].as_array()).map(Vec::as_slice).unwrap_or(&[])
}

fn current(s: &App) -> Option<View> {
    let ui = &s.ui.shell.sidebar.provider_updates;
    view(providers(s), ui.visible_after.as_deref(), &ui.dismissed)
}

/// Pins the "since" mark on first sight of the providers and retires a success after 3 s.
pub fn poll(s: &mut App) -> bool {
    let now = crate::data::now_ms();
    if s.ui.shell.sidebar.provider_updates.visible_after.is_none() {
        let latest = providers(s).iter().filter_map(|p| p["checkedAt"].as_str()).max().map(str::to_owned);
        if latest.is_none() {
            return false;
        }
        s.ui.shell.sidebar.provider_updates.visible_after = latest;
    }
    let view = current(s);
    let ui = &mut s.ui.shell.sidebar.provider_updates;
    match view.filter(|v| v.tone == Tone::Success) {
        Some(v) => match &ui.shown {
            Some((key, at)) if *key == v.key => {
                if now - at >= SUCCESS_VISIBLE_MS {
                    ui.dismissed.insert(v.key);
                    ui.shown = None;
                    return true;
                }
            }
            _ => {
                ui.shown = Some((v.key, now));
                if let Some(waker) = s.ui.panel.waker.clone() {
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(SUCCESS_VISIBLE_MS as u64));
                        waker();
                    });
                }
            }
        },
        None => ui.shown = None,
    }
    false
}

pub fn pill(s: &App, t: &T) -> Option<El> {
    let v = current(s)?;
    let (fill, fg) = match v.tone {
        Tone::Loading | Tone::Success => (t.sb.sidebar_control_surface, t.sb.sidebar_foreground),
        Tone::Warning => (t.sb.warning.alpha_mul(0.12), t.sb.warning),
        Tone::Error => (t.sb.destructive.alpha_mul(0.12), t.sb.destructive),
    };
    let glyph = match v.tone {
        Tone::Loading => &i::LOADER_CIRCLE,
        Tone::Success => &i::CIRCLE_CHECK,
        Tone::Error => &i::TRIANGLE_ALERT,
        Tone::Warning => &i::DOWNLOAD,
    };
    let main = row()
        .key("sb-provider-update-main")
        .role("button")
        .label(v.description.clone())
        .help(v.description.clone())
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .px(8.0)
        .py(6.0)
        .gap(8.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .child(icon(glyph, 14.0).color(fg).shrink())
        .child(text(v.title.clone()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM)).color(fg).wrap().shrink().min_w(0.0))
        .on_press(|s: &mut App| super::go(s, Route::Settings("providers".into())));
    let mut e = row().key("sb-provider-update").w_full().min_h(28.0).radius(8.0).clip_rounded(8.0).center_y().fill(fill).hover(fill.alpha_mul(1.5)).child(main);
    if v.dismissible {
        let key = v.key.clone();
        e = e.child(
            el().key("sb-provider-update-dismiss")
                .role("button")
                .label("Dismiss provider update notice")
                .help("Dismiss until provider status changes")
                .size(20.0, 20.0)
                .mr(4.0)
                .radius(6.0)
                .center()
                .shrink()
                .opacity(0.7)
                .cursor(Cursor::Pointer)
                .child(icon(&i::X, 14.0).color(fg))
                .on_press(move |s: &mut App| {
                    s.ui.shell.sidebar.provider_updates.dismissed.insert(key.clone());
                }),
        );
    }
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn p(driver: &str, status: &str, finished: Option<&str>) -> Value {
        json!({"driver": driver, "instanceId": driver, "checkedAt": "2026-11-08T10:00:00Z", "version": "1.2.0",
            "updateState": {"status": status, "finishedAt": finished}})
    }

    #[test]
    fn running_updates_win_then_the_newest_result_shows_until_dismissed() {
        let none = HashSet::new();
        let running = [p("codex", "running", None), p("claudeAgent", "failed", Some("2026-11-08T10:05:00Z"))];
        let v = view(&running, None, &none).unwrap();
        assert_eq!((v.tone, v.title.as_str()), (Tone::Loading, "Updating Codex"));

        let done = [p("codex", "succeeded", Some("2026-11-08T10:06:00Z")), p("claudeAgent", "failed", Some("2026-11-08T10:05:00Z"))];
        let v = view(&done, Some("2026-11-08T10:00:00Z"), &none).unwrap();
        assert_eq!((v.tone, v.title.as_str()), (Tone::Success, "Codex updated: v1.2.0"), "the newest result leads");
        let dismissed: HashSet<String> = [v.key].into();
        let v = view(&done, Some("2026-11-08T10:00:00Z"), &dismissed).unwrap();
        assert_eq!((v.tone, v.title.as_str()), (Tone::Error, "Claude update failed"));

        assert_eq!(view(&done, Some("2026-11-08T11:00:00Z"), &none), None, "results from before the window are hidden");
        let two = [p("codex", "queued", None), p("opencode", "running", None)];
        assert_eq!(view(&two, None, &none).unwrap().description, "Codex and OpenCode updates are in progress.");
    }
}
