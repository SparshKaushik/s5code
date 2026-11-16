//! What the pull request detail's header offers and how its actions report back
//! (PullRequestDetailPanel, PullRequestActionsMenu, PullRequestDetailActions): which menu items
//! show, the merge method, the stack membership gate, and the failure and unavailable texts.

use serde_json::Value;

use super::pr_detail::{Primary, allowed_methods, can, method_label};
use super::pr_row::{capitalized, term};
use crate::App;
use crate::data::s as field;

/// The last merge method picked anywhere (uiStateStore's `pullRequestMergeMethod`).
pub const LAST_MERGE_METHOD: &str = "pullRequestMergeMethod";

/// resolvePullRequestMergeMethod: the pick, the project's default, then the last one used, among
/// the methods the repository allows.
pub fn resolve_merge_method(d: &Value, current: Option<&str>, project: Option<&str>, last: Option<&str>) -> String {
    let allowed = allowed_methods(d);
    current.filter(|m| allowed.iter().any(|a| a == m)).map(str::to_string).or_else(|| super::pr_row::merge_method(d, project, last)).unwrap_or_else(|| "merge".into())
}

/// The project's `pullRequestMergeMethod`, else the environment's.
pub fn project_merge_method<'a>(settings: &'a Value, project_id: &str) -> Option<&'a str> {
    settings["projectSettingsOverrides"][project_id]["pullRequestMergeMethod"].as_str().or_else(|| settings["pullRequestMergeMethod"].as_str())
}

/// What a pull request's own pick falls back to: the project's default, then the last one used.
pub fn merge_defaults<'a>(s: &'a App, project_id: &str) -> (Option<&'a str>, Option<&'a str>) {
    let project = project_merge_method(crate::settings::server_settings(s), project_id);
    (project, s.client_settings.values.get(LAST_MERGE_METHOD).and_then(Value::as_str))
}

/// pullRequestActionNeedsHostRefresh: approving workflows changes checks the host has cached.
pub fn needs_host_refresh(action: &str) -> bool {
    action == "approve-workflows"
}

/// `Auto-merge (squash and merge)`, or plain "Auto-merge" when the method is unknown.
pub fn auto_merge_label(method: Option<&str>) -> String {
    method.map_or_else(|| "Auto-merge".into(), |m| format!("Auto-merge ({})", method_label(m).to_lowercase()))
}

/// The change request's `shortLabel`: GitLab's "MR", else "PR".
pub fn short_label(d: &Value) -> &'static str {
    if field(d, "url").contains("/-/merge_requests/") { "MR" } else { "PR" }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shows {
    pub draft_toggle: bool,
    pub auto_merge: bool,
    pub merge_now: bool,
    pub merge_methods: bool,
    pub delete_branch: bool,
}

impl Shows {
    /// pullRequestActionMenuHasGroup.
    pub fn group(self) -> bool {
        self.draft_toggle || self.auto_merge || self.merge_now || self.merge_methods
    }
}

/// The actions menu's optional items (showsDraftToggle, showsAutoMerge, showsMergeNow,
/// showsMergeMethods, canDeletePullRequestBranch).
pub fn shows(d: &Value, primary: Option<Primary>, single_merge: bool) -> Shows {
    let open = field(d, "state") == "open";
    let draft = d["isDraft"] == true;
    let conflicting = open && field(d, "mergeability") == "conflicting";
    let armed = open && d["autoMergeEnabled"] == true;
    let methods = allowed_methods(d).len();
    let mergeable = !draft && !conflicting && methods > 0;
    Shows {
        draft_toggle: open && can(d, if draft { "ready" } else { "draft" }) && !(draft && primary == Some(Primary::Ready)),
        auto_merge: single_merge
            && open
            && ((armed && can(d, "disable-auto-merge")) || (!armed && primary != Some(Primary::EnableAutoMerge) && mergeable && can(d, "enable-auto-merge"))),
        merge_now: single_merge && open && matches!(primary, Some(Primary::EnableAutoMerge | Primary::Armed)) && can(d, "merge") && mergeable,
        merge_methods: open && can(d, "merge") && !draft && !conflicting && methods > 1,
        delete_branch: !open && d["capabilities"]["deleteBranch"] == true && can(d, "delete-branch"),
    }
}

/// showsApproveWorkflows, without the list's fresher checks rollup, which this panel doesn't read.
pub fn shows_approve(d: &Value) -> bool {
    field(d, "state") == "open" && d["workflowApprovalsRequired"].as_u64().unwrap_or(0) > 0 && can(d, "approve-workflows")
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackGate {
    /// allowsSinglePullRequestMerge.
    pub single: bool,
    /// stackOwnsMerge: the stack menu merges, not this header.
    pub owns: bool,
    /// mergeBlockedReason.
    pub blocked: Option<&'static str>,
    /// The lookup failed with no stack held, so the header offers to retry it.
    pub failed: bool,
}

pub fn gate(supports: bool, has_stack: bool, pending: bool, failed: bool) -> StackGate {
    let single = !supports || (!has_stack && !pending && !failed);
    StackGate {
        single,
        owns: supports && has_stack,
        blocked: (!single).then_some(if failed { "Couldn't check stack membership" } else { "Checking stack membership…" }),
        failed: supports && failed && !has_stack,
    }
}

fn supports_stack_actions(d: &Value) -> bool {
    d["capabilities"]["stacks"] == true && d["capabilities"]["stackActions"] == true
}

/// The gate over the row's `pullRequests.stack` read, which the list rows share.
pub fn stack_gate(s: &App, d: &Value) -> StackGate {
    let read = s.ui.pages.detail.entry.as_ref().and_then(|e| s.ui.pages.pull_requests.row.stacks.get(&super::pr_row::row_key(e)));
    let has_stack = read.and_then(|r| r.data.as_ref()).is_some_and(Value::is_object);
    let failed = read.is_some_and(|r| r.error.is_some());
    let pending = read.is_none_or(|r| r.data.is_none() && r.error.is_none());
    gate(supports_stack_actions(d), has_stack, pending, failed)
}

/// Reads the open pull request's stack when its host merges stacks itself.
pub fn want_stack(s: &mut App) {
    let u = &s.ui.pages.detail;
    let (Some(entry), Some(d)) = (&u.entry, &u.detail) else { return };
    if !supports_stack_actions(d) {
        return;
    }
    let entry = entry.clone();
    super::pr_row::want(s, super::pr_row::Kind::Stack, &entry);
}

/// actionFailureLabels and actionFailureHints.
pub fn failure_text(action: &str, term: &str) -> (String, String) {
    let (title, hint) = match action {
        "merge" => (
            format!("Could not merge this {term}"),
            "The host refused the merge. Check that you have write access, that the checks it requires have passed, and that the branch is not conflicting.".to_string(),
        ),
        "ready" => ("Could not mark this ready for review".into(), "The host refused it. Check that you have write access to this repository.".into()),
        "draft" => ("Could not convert this to a draft".into(), "The host refused it. Check that you have write access to this repository.".into()),
        "close" => (format!("Could not close this {term}"), "The host refused it. Check that you have write access, or that you opened it.".into()),
        "reopen" => (format!("Could not reopen this {term}"), "The host refused it. Check that you have write access, and that the branch still exists.".into()),
        "update-branch" => (
            "Could not update this branch".into(),
            "The host refused it. Check that you have write access to the branch (a fork branch also needs its author to allow edits from maintainers), and that it does not conflict with the base.".into(),
        ),
        "enable-auto-merge" => (
            "Could not turn on auto-merge".into(),
            "The host refused it. Check that this repository allows auto-merge, that you have write access, and that there is something left for it to wait on.".into(),
        ),
        "disable-auto-merge" => ("Could not turn off auto-merge".into(), "The host refused it. Check that you have write access, and that the merge has not already happened.".into()),
        "revert" => (format!("Could not open a revert {term}"), format!("The host refused it. Check that you have write access and that this {term} was merged on the host.")),
        "delete-branch" => (
            "Could not delete this branch".into(),
            "The host refused it. Check that you have write access to the source repository and that the branch has not moved since this page loaded.".into(),
        ),
        _ => (
            "Could not approve workflows".into(),
            "The host refused it. Check that you have Actions write access and that these workflow runs are still awaiting approval.".into(),
        ),
    };
    (title, hint)
}

/// The toast a finished action raises; the others show in the refreshed detail.
pub fn success_title(action: &str, term: &str) -> Option<String> {
    match action {
        "revert" => Some(format!("Revert {term} opened")),
        "delete-branch" => Some("Branch deleted".into()),
        _ => None,
    }
}

/// isPullRequestNotFound: an operation error the host marked `not-found`.
pub fn not_found(cause: &Value) -> bool {
    match cause {
        Value::Object(m) if m.get("_tag").and_then(Value::as_str) == Some("PullRequestOperationError") => m.get("reason").and_then(Value::as_str) == Some("not-found"),
        Value::Object(m) => m.values().any(not_found),
        Value::Array(rows) => rows.iter().any(not_found),
        _ => false,
    }
}

/// PullRequestsUnavailableState's title and description for a detail that didn't load.
pub fn unavailable_text(entry: &Value, not_found: bool, error: &str) -> (String, String) {
    if !not_found {
        return ("Could not load pull requests".into(), error.to_string());
    }
    let term = term(entry);
    (format!("{} #{} not found", capitalized(term), entry["number"]), format!("It may be an issue rather than a {term}, or this account can't see it."))
}

/// confirmDeleteBranch's question and detail.
pub fn delete_branch_prompt(d: &Value) -> (String, String) {
    let branch = field(d, "headBranch");
    let repository = d["headRepositoryNameWithOwner"].as_str().unwrap_or(field(d, "repository"));
    (format!("Delete branch {branch}?"), format!("This deletes {branch} from {repository}. Local branches and worktrees are not changed."))
}

/// describePullRequestDiffError: a rate limit is named, anything else reads as a failed load.
pub fn describe_diff_error(error: &str, provider: &str, term: &str) -> (String, String) {
    let lower = error.to_lowercase();
    if ["rate limit", "rate-limited", "rate_limited", "too many requests"].iter().any(|p| has_word(&lower, p)) {
        return (
            format!("{provider} rate limit reached"),
            format!("{provider} is temporarily limiting requests, so this diff couldn't be loaded. Try again after the limit resets, or view the changes on {provider}."),
        );
    }
    ("Couldn't load the changes".into(), format!("The environment couldn't load this {term}'s diff. Try again, or view the changes on {provider}."))
}

/// `\b{phrase}\b`.
fn has_word(hay: &str, phrase: &str) -> bool {
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
    hay.match_indices(phrase).any(|(at, _)| !word(hay[..at].chars().next_back()) && !word(hay[at + phrase.len()..].chars().next()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ALL: [&str; 11] = ["merge", "ready", "draft", "close", "reopen", "update-branch", "enable-auto-merge", "disable-auto-merge", "revert", "delete-branch", "approve-workflows"];

    fn pr(patch: Value) -> Value {
        let mut d = json!({
            "state": "open",
            "isDraft": false,
            "mergeability": "mergeable",
            "url": "https://github.com/o/r/pull/7",
            "provider": "github",
            "number": 7,
            "repository": "o/r",
            "headBranch": "fix/clamp",
            "autoMergeEnabled": false,
            "capabilities": {"actions": ALL, "mergeMethods": ["squash", "merge", "rebase"], "deleteBranch": true},
            "viewerPermissions": {"actions": ALL},
            "mergeCapabilities": {"merge": true, "squash": true, "rebase": true},
        });
        for (k, v) in patch.as_object().unwrap() {
            d[k] = v.clone();
        }
        d
    }

    #[test]
    fn the_menu_offers_what_the_primary_control_does_not() {
        let open = pr(json!({}));
        let s = shows(&open, Some(Primary::Merge), true);
        assert_eq!(s, Shows { draft_toggle: true, auto_merge: true, merge_now: false, merge_methods: true, delete_branch: false });
        assert!(s.group());

        // Auto-merge leads, so the menu offers merging now instead of enabling it again.
        let s = shows(&open, Some(Primary::EnableAutoMerge), true);
        assert_eq!((s.auto_merge, s.merge_now), (false, true));

        let armed = pr(json!({"autoMergeEnabled": true}));
        let s = shows(&armed, Some(Primary::Armed), true);
        assert_eq!((s.auto_merge, s.merge_now), (true, true), "disable, and merge without waiting");

        // A stack that may own the merge hides both until its membership is known.
        let s = shows(&armed, Some(Primary::Armed), false);
        assert_eq!((s.auto_merge, s.merge_now, s.merge_methods), (false, false, true));

        let draft = pr(json!({"isDraft": true}));
        let s = shows(&draft, Some(Primary::Ready), true);
        assert_eq!(s, Shows::default(), "Ready leads, and a draft neither merges nor arms");
        assert!(shows(&draft, None, true).draft_toggle);

        let conflicting = pr(json!({"mergeability": "conflicting"}));
        let s = shows(&conflicting, Some(Primary::Resolve), true);
        assert_eq!((s.draft_toggle, s.auto_merge, s.merge_methods), (true, false, false));

        let one_method = pr(json!({"mergeCapabilities": {"merge": false, "squash": true, "rebase": false}}));
        assert!(!shows(&one_method, Some(Primary::Merge), true).merge_methods);

        for state in ["merged", "closed"] {
            let s = shows(&pr(json!({"state": state})), None, true);
            assert_eq!(s, Shows { delete_branch: true, ..Shows::default() });
            assert!(!s.group());
        }
        let no_delete = pr(json!({"state": "merged", "capabilities": {"actions": ALL, "mergeMethods": [], "deleteBranch": false}}));
        assert!(!shows(&no_delete, None, true).delete_branch);

        assert!(!shows_approve(&open));
        assert!(shows_approve(&pr(json!({"workflowApprovalsRequired": 2}))));
        assert!(!shows_approve(&pr(json!({"workflowApprovalsRequired": 2, "state": "closed"}))));
    }

    #[test]
    fn the_merge_method_follows_the_pick_then_project_then_last_used() {
        let d = pr(json!({}));
        assert_eq!(resolve_merge_method(&d, None, None, None), "squash");
        assert_eq!(resolve_merge_method(&d, Some("rebase"), Some("merge"), Some("squash")), "rebase");
        assert_eq!(resolve_merge_method(&d, None, Some("merge"), Some("rebase")), "merge");
        assert_eq!(resolve_merge_method(&d, None, None, Some("rebase")), "rebase");
        let no_rebase = pr(json!({"mergeCapabilities": {"merge": true, "squash": true, "rebase": false}}));
        assert_eq!(resolve_merge_method(&no_rebase, Some("rebase"), None, Some("merge")), "merge");
        assert_eq!(resolve_merge_method(&pr(json!({"mergeCapabilities": {}})), Some("squash"), None, None), "merge");

        let settings = json!({"pullRequestMergeMethod": "rebase", "projectSettingsOverrides": {"p": {"pullRequestMergeMethod": "merge"}}});
        assert_eq!(project_merge_method(&settings, "p"), Some("merge"));
        assert_eq!(project_merge_method(&settings, "q"), Some("rebase"));
        assert_eq!(project_merge_method(&json!({}), "p"), None);

        assert_eq!(auto_merge_label(Some("squash")), "Auto-merge (squash and merge)");
        assert_eq!(auto_merge_label(Some("merge")), "Auto-merge (merge)");
        assert_eq!(auto_merge_label(None), "Auto-merge");
    }

    #[test]
    fn a_stacked_host_holds_the_merge_until_membership_is_known() {
        assert_eq!(gate(false, false, true, false), StackGate { single: true, owns: false, blocked: None, failed: false });
        assert_eq!(gate(true, false, false, false), StackGate { single: true, owns: false, blocked: None, failed: false });
        assert_eq!(gate(true, false, true, false).blocked, Some("Checking stack membership…"));
        let failed = gate(true, false, false, true);
        assert_eq!((failed.single, failed.blocked, failed.failed), (false, Some("Couldn't check stack membership"), true));
        let stacked = gate(true, true, false, false);
        assert_eq!((stacked.single, stacked.owns, stacked.failed), (false, true, false));
    }

    #[test]
    fn failures_and_outcomes_read_in_the_hosts_terms() {
        assert_eq!(failure_text("revert", "merge request").0, "Could not open a revert merge request");
        assert!(failure_text("revert", "merge request").1.ends_with("this merge request was merged on the host."));
        assert_eq!(failure_text("enable-auto-merge", "pull request").0, "Could not turn on auto-merge");
        assert_eq!(failure_text("approve-workflows", "pull request").0, "Could not approve workflows");
        assert_eq!(failure_text("delete-branch", "pull request").0, "Could not delete this branch");
        assert_eq!(success_title("revert", "pull request").as_deref(), Some("Revert pull request opened"));
        assert_eq!(success_title("delete-branch", "pull request").as_deref(), Some("Branch deleted"));
        assert_eq!(success_title("merge", "pull request"), None);
        assert!(ALL.iter().all(|a| needs_host_refresh(a) == (*a == "approve-workflows")), "only approving workflows rereads the host");

        let missing = json!({"cause": {"_tag": "Fail", "error": {"_tag": "PullRequestOperationError", "operation": "detail", "detail": "Not found", "reason": "not-found"}}});
        assert!(not_found(&missing));
        assert!(!not_found(&json!({"_tag": "PullRequestOperationError", "operation": "detail", "detail": "boom"})));
        let entry = json!({"provider": "gitlab", "number": 9});
        assert_eq!(
            unavailable_text(&entry, true, "x"),
            ("Merge request #9 not found".to_string(), "It may be an issue rather than a merge request, or this account can't see it.".to_string())
        );
        assert_eq!(unavailable_text(&entry, false, "boom"), ("Could not load pull requests".to_string(), "boom".to_string()));

        let d = pr(json!({"headRepositoryNameWithOwner": "fork/r"}));
        assert_eq!(delete_branch_prompt(&d).1, "This deletes fix/clamp from fork/r. Local branches and worktrees are not changed.");
        assert_eq!(delete_branch_prompt(&pr(json!({}))).0, "Delete branch fix/clamp?");

        assert_eq!(describe_diff_error("HTTP 429: Too Many Requests", "GitHub", "pull request").0, "GitHub rate limit reached");
        assert_eq!(describe_diff_error("API rate_limited", "GitLab", "merge request").0, "GitLab rate limit reached");
        assert_eq!(
            describe_diff_error("generate rate limiter", "GitHub", "pull request"),
            ("Couldn't load the changes".to_string(), "The environment couldn't load this pull request's diff. Try again, or view the changes on GitHub.".to_string())
        );
    }
}
