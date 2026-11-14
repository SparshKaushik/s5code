//! Web's client permission gating: the connection's session (`GET /api/auth/session`) checked
//! against contracts `CLIENT_GUARDED_RPC_SCOPES`. The server still authorizes every request.
use crate::{App, theme::T};
use rune::El;
use serde_json::{Value, json};

#[derive(Default)]
pub struct Session {
    reply: Option<Result<Value, Value>>,
    pending: Option<u64>,
    /// The inert control under the pointer: its row key and the reason it is inert.
    tip: Option<(String, String)>,
}

/// Refetches the session; the last answer stays readable while the new one is in flight.
pub fn load(s: &mut App) {
    if s.ui.settings.session.pending.is_some() {
        return;
    }
    let id = s.src.net().map(|net| net.http_request("GET", "/api/auth/session", json!({})));
    s.ui.settings.session.pending = id;
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.settings.session.pending else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    let session = &mut s.ui.settings.session;
    session.pending = None;
    session.reply = Some(reply);
    true
}

/// The session as the server last answered it.
pub fn session(s: &App) -> Option<&Value> {
    s.ui.settings.session.reply.as_ref()?.as_ref().ok()
}

/// Web `useEnvironmentScope`: denied until the session answers, and when it fails. Fixtures
/// have no server and grant everything.
pub fn can(s: &App, scope: &str) -> bool {
    match &s.src {
        crate::data::Source::Fixture(_) => true,
        crate::data::Source::Live(_) => session(s).is_some_and(|session| grants(session, scope)),
    }
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
    let label = || crate::env::label(s, crate::env::LOCAL);
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
        .child(control.disabled(true).opacity(0.5))
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
