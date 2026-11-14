//! Web's client permission gating: each environment's session (`GET /api/auth/session`)
//! checked against contracts `CLIENT_GUARDED_RPC_SCOPES`, for the environment a control acts on.
//! The server still authorizes every request.
use crate::{App, theme::T};
use rune::El;
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Session {
    /// Each environment's last answer and the request in flight, by environment key.
    by_env: HashMap<String, Entry>,
    /// The inert control under the pointer: its row key and the reason it is inert.
    tip: Option<(String, String)>,
}

#[derive(Default)]
struct Entry {
    reply: Option<Result<Value, Value>>,
    pending: Option<u64>,
}

/// Refetches every connected environment's session; last answers stay readable meanwhile.
pub fn load(s: &mut App) {
    for key in crate::env::keys(s) {
        fetch(s, &key);
    }
}

fn fetch(s: &mut App, key: &str) {
    if s.ui.settings.session.by_env.get(key).is_some_and(|e| e.pending.is_some()) {
        return;
    }
    let Some(id) = crate::env::net_of(s, key).map(|net| net.http_request("GET", "/api/auth/session", json!({}))) else { return };
    s.ui.settings.session.by_env.entry(key.to_owned()).or_default().pending = Some(id);
}

pub fn poll(s: &mut App) -> bool {
    // An environment switched to, or connected since, is asked once.
    let target = target(s);
    for key in crate::env::keys(s) {
        if !s.ui.settings.session.by_env.contains_key(&key) && (key == target || crate::env::connected(s, &key)) {
            fetch(s, &key);
        }
    }
    let pending: Vec<(String, u64)> = s.ui.settings.session.by_env.iter().filter_map(|(k, e)| Some((k.clone(), e.pending?))).collect();
    let mut changed = false;
    for (key, id) in pending {
        let Some(reply) = crate::env::net_of(s, &key).and_then(|net| net.take_reply(id)) else { continue };
        let entry = s.ui.settings.session.by_env.entry(key).or_default();
        entry.pending = None;
        entry.reply = Some(reply);
        changed = true;
    }
    changed
}

/// The environment the settings on screen write to: the scoped project's, else the one on screen.
pub fn target(s: &App) -> String {
    match &s.ui.settings.project {
        Some(project) => crate::env::owner(s, project),
        None => s.ui.env.active.clone(),
    }
}

/// The last answer to the target environment's session request, failed or not.
pub fn reply(s: &App) -> Option<&Result<Value, Value>> {
    reply_on(s, &target(s))
}

fn reply_on<'a>(s: &'a App, key: &str) -> Option<&'a Result<Value, Value>> {
    s.ui.settings.session.by_env.get(key)?.reply.as_ref()
}

/// Web `useEnvironmentScope` for the target environment.
pub fn can(s: &App, scope: &str) -> bool {
    can_on(s, &target(s), scope)
}

/// Web `useEnvironmentScope(environmentId, scope)`: denied until that session answers, and when
/// it fails. Fixtures have no server and grant everything.
pub fn can_on(s: &App, key: &str, scope: &str) -> bool {
    match crate::env::source(s, key) {
        Some(crate::data::Source::Fixture(_)) => true,
        Some(crate::data::Source::Live(_)) => reply_on(s, key).and_then(|r| r.as_ref().ok()).is_some_and(|session| grants(session, scope)),
        None => false,
    }
}

/// Web `resolveFilesystemReadAccess`, for any scope: `None` while the session check runs, else
/// granted, or why not (`Err(None)`: denied, in the caller's own words).
pub fn access_on(s: &App, key: &str, scope: &str) -> Option<Result<(), Option<String>>> {
    const OFFLINE: &str = "This environment is not connected.";
    match crate::env::source(s, key) {
        Some(crate::data::Source::Fixture(_)) => Some(Ok(())),
        Some(crate::data::Source::Live(_)) => match reply_on(s, key) {
            Some(Ok(session)) => Some(if grants(session, scope) { Ok(()) } else { Err(None) }),
            Some(Err(cause)) => Some(Err(Some(crate::shell::palette::error_message(cause, OFFLINE)))),
            None if crate::env::connected(s, key) => None,
            None => Some(Err(Some(OFFLINE.into()))),
        },
        None => Some(Err(Some(OFFLINE.into()))),
    }
}

/// `access_on` with the caller's denial copy: `None` while checking.
pub fn access_or(s: &App, key: &str, scope: &str, denied: &str) -> Option<Result<(), String>> {
    access_on(s, key, scope).map(|r| r.map_err(|reason| reason.unwrap_or_else(|| denied.to_owned())))
}

/// Every scope `method` needs (contracts `clientRpcRequiredScopes`) is granted.
pub fn allowed(s: &App, method: &str) -> bool {
    required(method).is_none_or(|scope| can(s, scope))
}

pub fn can_write_settings(s: &App) -> bool {
    can(s, "settings:write")
}

/// Contracts `sessionGrantsScope`, with its legacy parents.
pub fn grants(session: &Value, scope: &str) -> bool {
    if session["authenticated"] != true {
        return false;
    }
    let has = |list: &Value, scope: &str| list.as_array().is_some_and(|l| l.iter().any(|s| s == scope));
    if session["permissions"].is_array() {
        return has(&session["permissions"], scope);
    }
    if has(&session["scopes"], scope) {
        return true;
    }
    if !session["auth"]["serverUpdateScope"].is_null() {
        return false;
    }
    let parent = match scope {
        "filesystem:read" | "diagnostics:read" => "orchestration:read",
        "settings:write" | "providers:manage" | "environment:maintain" | "preview:operate" | "source-control:write"
        | "filesystem:write" => "orchestration:operate",
        "terminal:read" => "terminal:operate",
        _ => return false,
    };
    has(&session["scopes"], parent)
}

/// Contracts `CLIENT_GUARDED_RPC_SCOPES`, keyed by wire method.
pub fn required(method: &str) -> Option<&'static str> {
    Some(match method {
        "preview.close" => "preview:operate",
        "preview.stopLocalServer" => "terminal:operate",
        "server.runStorageCleanup" | "server.checkOtlpEndpoint" => "settings:write",
        "sourceControl.signInStart" | "sourceControl.signInWait" | "sourceControl.signInCancel" => "settings:write",
        "server.refreshProviders" => "orchestration:read",
        "pullRequests.runAction"
        | "pullRequests.update"
        | "pullRequests.comment"
        | "pullRequests.updateComment"
        | "pullRequests.submitReview"
        | "pullRequests.replyToThread"
        | "pullRequests.setThreadResolution"
        | "pullRequests.setReaction"
        | "pullRequests.setFilesViewed"
        | "pullRequests.requestReviewers"
        | "pullRequests.setLabels"
        | "sourceControl.cloneRepository"
        | "sourceControl.publishRepository"
        | "sourceControl.star"
        | "projectClone.start"
        | "projectClone.cancel"
        | "projectClone.retry"
        | "vcs.pull"
        | "git.runStackedAction"
        | "git.preparePullRequestThread"
        | "vcs.createWorktree"
        | "vcs.removeWorktree"
        | "vcs.createRef"
        | "vcs.switchRef"
        | "vcs.init" => "source-control:write",
        "pullRequests.generateReviewGuide"
        | "projects.createFromThread"
        | "server.prepareTriage"
        | "scheduledTasks.upsert"
        | "scheduledTasks.setEnabled"
        | "scheduledTasks.delete"
        | "scheduledTasks.runNow"
        | "scheduledTasks.rotateWebhookToken"
        | "orchestration.stopWorkflow"
        | "orchestration.forceStopThread" => "orchestration:operate",
        "guestShare.create" | "peerLinks.link" | "peerLinks.unlink" | "peerLinks.answerRequest" => "access:write",
        _ => return None,
    })
}

/// Web `SettingsRow`'s reason a server-saved row's control is inert, if it is.
pub fn row_lock(s: &App, key: &str) -> Option<String> {
    let label = || crate::env::label(s, &target(s));
    lock_message(
        can_write_settings(s),
        locked_by(s.state().config.as_ref().map(|c| &c["managedPolicy"]), key),
        &label,
    )
}

fn lock_message(can_write: bool, managed_by: Option<&str>, label: &dyn Fn() -> String) -> Option<String> {
    if let Some(org) = managed_by {
        return Some(format!("Managed by {org}."));
    }
    (!can_write).then(|| format!("This connection lacks permission to change settings on {}.", label()))
}

/// Contracts `isManagedSettingPath`: the organization when `path` is locked, inside a locked
/// setting, or contains one.
fn locked_by<'a>(policy: Option<&'a Value>, path: &str) -> Option<&'a str> {
    let policy = policy?;
    let locked = policy["lockedSettings"].as_array()?.iter().filter_map(Value::as_str).any(|locked| {
        locked == path || locked.strip_prefix(path).is_some_and(|r| r.starts_with('.')) || path.strip_prefix(locked).is_some_and(|r| r.starts_with('.'))
    });
    locked.then(|| policy["organization"].as_str().unwrap_or("your organization"))
}

/// Web's `inertControl`: the control at half opacity, unreachable, with `message` on hover.
/// `key` must be unique on the page; [`overlays`] mounts the tooltip.
pub fn inert(key: &str, control: El, message: String) -> El {
    let owned = key.to_owned();
    rune::row()
        .key(format!("inert-{key}-trigger"))
        .on_hover_change(move |s: &mut App, inside, _| {
            let tip = &mut s.ui.settings.session.tip;
            if inside {
                *tip = Some((owned.clone(), message.clone()));
            } else if tip.as_ref().is_some_and(|(k, _)| *k == owned) {
                *tip = None;
            }
        })
        .child(control.disabled(true).inert().opacity(0.5))
}

/// `inert` when `message` is set, else the control as-is.
pub fn gate(key: &str, control: El, message: Option<String>) -> El {
    match message {
        Some(message) => inert(key, control, message),
        None => control,
    }
}

/// The message for a control whose RPC this connection may not call.
pub fn method_lock(s: &App, method: &str) -> Option<String> {
    (!allowed(s, method)).then(|| "This connection doesn't have permission to do this.".to_owned())
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    if let Some((key, message)) = &s.ui.settings.session.tip {
        let _ = crate::kit::Tooltip::new(format!("inert-{key}"))
            .label(message.clone())
            .open(true)
            .build(&crate::kit::theme(t), layers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_methods_need_their_contract_scope() {
        assert_eq!(required("server.runStorageCleanup"), Some("settings:write"));
        assert_eq!(required("scheduledTasks.runNow"), Some("orchestration:operate"));
        assert_eq!(required("peerLinks.link"), Some("access:write"));
        assert_eq!(required("projectClone.start"), Some("source-control:write"));
        assert_eq!(required("server.refreshProviders"), Some("orchestration:read"));
        assert_eq!(required("server.updateSettings"), None);
    }

    #[test]
    fn row_lock_prefers_the_managed_policy_over_a_missing_grant() {
        let policy = json!({"organization": "Example Corp", "lockedSettings": ["storageCleanup"]});
        assert_eq!(locked_by(Some(&policy), "storageCleanup.logsAfterDays"), Some("Example Corp"));
        assert_eq!(locked_by(Some(&policy), "storageCleanup"), Some("Example Corp"));
        assert_eq!(locked_by(Some(&policy), "storageCleanupX"), None);
        let nested = json!({"organization": "O", "lockedSettings": ["a.b"]});
        assert_eq!(locked_by(Some(&nested), "a"), Some("O"));
        let label = || "Laptop".to_owned();
        assert_eq!(lock_message(false, Some("Example Corp"), &label).as_deref(), Some("Managed by Example Corp."));
        assert_eq!(
            lock_message(false, None, &label).as_deref(),
            Some("This connection lacks permission to change settings on Laptop.")
        );
        assert_eq!(lock_message(true, None, &label), None);
    }
}
