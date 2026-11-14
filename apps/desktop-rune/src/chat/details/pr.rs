//! Pull requests in the details card: the "Checkout pull request" dialog the branch picker opens
//! (PullRequestThreadDialog) and the thread's PR rows (ThreadDetailsPrRows).

use rune::{Color, Cursor, El, TextEdit, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::git;
use crate::icons;
use crate::kit;
use crate::shell::palette;
use crate::theme::{self, AlphaMul, T, radius, tw};
use crate::{App, data};

#[derive(Default)]
pub struct Checkout {
    reference: TextEdit,
    /// The text as of the last poll, and when it last changed (the 450 ms resolve debounce).
    seen: String,
    edited_at: i64,
    dirty: bool,
    /// The resolved pull request and the parsed reference it answers.
    resolved: Option<(String, Value)>,
    resolving: Option<(u64, String)>,
    resolve_error: Option<(String, String)>,
    preparing: Option<(u64, Mode)>,
    error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Mode {
    Local,
    Worktree,
}

impl Mode {
    fn wire(self) -> &'static str {
        match self {
            Mode::Local => "local",
            Mode::Worktree => "worktree",
        }
    }
}

/// `parsePullRequestReference`: a checkout command's argument, a change request URL, or `123` /
/// `#123`; anything else is not a pull request.
pub fn parse_reference(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let arg = checkout_argument(trimmed).unwrap_or(trimmed);
    if arg.is_empty() {
        return None;
    }
    if is_change_request_url(arg) {
        return Some(arg.to_string());
    }
    let digits = arg.strip_prefix('#').unwrap_or(arg);
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.to_string())
}

/// `gh pr checkout 42`, `glab mr checkout 42`, `tea pr checkout 42`, `az repos pr checkout --id 42`.
fn checkout_argument(input: &str) -> Option<&str> {
    let words: Vec<&str> = input.split_whitespace().collect();
    let rest = match words.as_slice() {
        ["gh", "pr", "checkout", rest @ ..] | ["glab", "mr", "checkout", rest @ ..] => rest,
        ["tea", "pr" | "pulls", "checkout", rest @ ..] => rest,
        ["az", "repos", "pr", "checkout", rest @ ..] => rest,
        _ => return None,
    };
    rest.iter().rev().find(|w| !w.starts_with('-')).copied()
}

/// GitHub `/owner/repo/pull/N`, GitLab `/-/merge_requests/N`, Gitea/Forgejo `/pulls/N`, Azure
/// `/_git/repo/pullrequest/N`.
fn is_change_request_url(input: &str) -> bool {
    let lower = input.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")) else { return false };
    let path = rest.split(['?', '#']).next().unwrap_or("");
    let segments: Vec<&str> = path.split('/').skip(1).filter(|s| !s.is_empty()).collect();
    segments.windows(2).any(|w| {
        matches!(w[0], "pull" | "pulls" | "merge_requests" | "pullrequest") && !w[1].is_empty() && w[1].bytes().all(|b| b.is_ascii_digit())
    })
}

pub fn singular(s: &App) -> &'static str {
    git::terms(git::status(s).as_ref()).singular
}

fn provider_name(s: &App) -> &'static str {
    match git::status(s).as_ref().and_then(|v| v["sourceControlProvider"]["kind"].as_str()) {
        Some("gitlab") => "GitLab",
        Some("bitbucket") => "Bitbucket",
        Some("azure-devops") => "Azure DevOps",
        Some("gitcafe") => "GitCafe",
        Some("forgejo") => "Forgejo",
        Some("github") | None => "GitHub",
        Some(_) => "source control",
    }
}

/// Opens the dialog, with the picker's query when it named a pull request.
pub fn open_checkout(s: &mut App, reference: Option<String>) {
    let mut edit = TextEdit::default();
    if let Some(r) = &reference {
        edit.replace_text(r.clone());
        edit.set_selection(0, r.len());
    }
    edit.focused = true;
    let seen = edit.text.clone();
    s.ui.chat.details.checkout = Some(Checkout { reference: edit, seen, ..Checkout::default() });
}

/// The dialog with a resolved pull request, for headless scenes.
pub fn scene(s: &mut App) {
    open_checkout(s, Some("#42".into()));
    let pr = serde_json::json!({"number": 42, "title": "Queue keystrokes until the terminal attaches", "url": "https://github.com/acme/demo/pull/42", "headBranch": "fix/terminal-first-key", "baseBranch": "main", "state": "open"});
    if let Some(c) = s.ui.chat.details.checkout.as_mut() {
        c.resolved = parse_reference("#42").map(|r| (r, pr));
    }
}

fn close(s: &mut App) {
    if s.ui.chat.details.checkout.as_ref().is_some_and(|c| c.preparing.is_none()) {
        s.ui.chat.details.checkout = None;
    }
}

/// Resolves the typed reference 450 ms after the last edit, and takes the replies.
pub fn poll(s: &mut App) -> bool {
    let Some(c) = s.ui.chat.details.checkout.as_mut() else { return false };
    let now = data::now_ms();
    let mut changed = false;
    if c.reference.text != c.seen {
        c.seen = c.reference.text.clone();
        c.edited_at = now;
        c.dirty = true;
        c.error = None;
        changed = true;
    }
    if let Some((id, reference)) = c.resolving.clone()
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let c = s.ui.chat.details.checkout.as_mut().expect("checked above");
        c.resolving = None;
        match reply {
            Ok(v) => c.resolved = Some((reference, v["pullRequest"].clone())),
            Err(e) => c.resolve_error = Some((reference, palette::error_message(&e, "Failed to resolve pull request."))),
        }
        changed = true;
    }
    if let Some((id, _)) = s.ui.chat.details.checkout.as_ref().and_then(|c| c.preparing)
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        changed = true;
        match reply {
            Ok(v) => prepared(s, &v),
            Err(e) => {
                let fallback = format!("Failed to prepare {} thread.", singular(s));
                let c = s.ui.chat.details.checkout.as_mut().expect("checked above");
                c.preparing = None;
                c.error = Some(palette::error_message(&e, &fallback));
            }
        }
    }
    let Some(c) = s.ui.chat.details.checkout.as_ref() else { return changed };
    let wanted = parse_reference(&c.reference.text);
    let settled = data::now_ms() - c.edited_at >= 450 || c.edited_at == 0;
    if let Some(reference) = wanted.filter(|r| settled && c.resolving.is_none() && !answered(c, r))
        && let (Some(cwd), Some(net)) = (super::cwd(s), s.src.net())
    {
        let id = net.request("git.resolvePullRequest", json!({"cwd": cwd, "reference": reference}));
        s.ui.chat.details.checkout.as_mut().expect("checked above").resolving = Some((id, reference));
        changed = true;
    }
    changed
}

fn answered(c: &Checkout, reference: &str) -> bool {
    c.resolved.as_ref().is_some_and(|r| r.0 == reference) || c.resolve_error.as_ref().is_some_and(|e| e.0 == reference)
}

fn resolved(c: &Checkout) -> Option<&Value> {
    let reference = parse_reference(&c.reference.text)?;
    c.resolved.as_ref().filter(|r| r.0 == reference).map(|r| &r.1).filter(|v| v.is_object())
}

fn resolving(c: &Checkout) -> bool {
    let Some(reference) = parse_reference(&c.reference.text) else { return false };
    resolved(c).is_none() && c.resolve_error.as_ref().is_none_or(|e| e.0 != reference)
}

fn confirm(s: &mut App, mode: Mode) {
    let Some(c) = s.ui.chat.details.checkout.as_mut() else { return };
    if c.preparing.is_some() {
        return;
    }
    let Some(reference) = parse_reference(&c.reference.text) else {
        c.dirty = true;
        return;
    };
    if resolved(c).is_none() {
        return;
    }
    let mut params = json!({"reference": reference, "mode": mode.wire()});
    let (Some(cwd), Some(net)) = (super::cwd(s), s.src.net()) else { return };
    params["cwd"] = json!(cwd);
    let id = net.request("git.preparePullRequestThread", params);
    let c = s.ui.chat.details.checkout.as_mut().expect("checked above");
    c.error = None;
    c.preparing = Some((id, mode));
}

/// `usePullRequestDraftThread`: the prepared branch, in its worktree when there is one, becomes
/// the project's draft.
fn prepared(s: &mut App, v: &Value) {
    let branch = data::s(v, "branch").to_string();
    let worktree = v["worktreePath"].as_str().map(str::to_string);
    let on_head = v["isOnPullRequestHead"] != false;
    let singular = singular(s);
    s.ui.chat.details.checkout = None;
    if let Some(project) = super::composer::ctx(s).project.map(|p| data::s(p, "id").to_string()) {
        crate::chat::home::start_on_branch(s, &project, &branch, worktree.as_deref());
    }
    if !on_head {
        palette::toast(
            s,
            "Checked out, but not confirmed on the latest commit",
            Some(&format!(
                "This checkout could not be confirmed at the {singular}'s latest commit, so its code may differ. Local commits or uncommitted work keep a checkout where it is."
            )),
        );
    }
}

/// `pullRequestStatusTone`.
fn state_color(t: &T, state: &str) -> Color {
    match (state, t.dark) {
        ("merged", true) => tw::VIOLET_300.alpha_mul(0.9),
        ("merged", false) => tw::VIOLET_600,
        ("closed", true) => tw::ZINC_400.alpha_mul(0.8),
        ("closed", false) => tw::ZINC_500,
        ("open", true) => tw::EMERALD_300.alpha_mul(0.9),
        ("open", false) => tw::EMERALD_600,
        _ => t.c.muted_foreground,
    }
}

pub fn overlays(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(c) = s.ui.chat.details.checkout.as_ref() else { return };
    let k = kit::theme(t);
    let muted = t.c.muted_foreground;
    let singular = singular(s);
    let short = git::terms(git::status(s).as_ref()).short;
    let parsed = parse_reference(&c.reference.text);
    let pr = resolved(c);
    let busy = resolving(c);
    let input = kit::Input::new(&c.reference)
        .placeholder(format!("{short} URL, checkout command, or #42"))
        .disabled(c.preparing.is_some())
        .on_edit(|s: &mut App| &mut s.ui.chat.details.checkout.as_mut().expect("dialog is open").reference)
        .on_submit(|s: &mut App, _: &mut Tx| {
            if s.ui.chat.details.checkout.as_ref().is_some_and(|c| !resolving(c)) {
                confirm(s, Mode::Worktree);
            }
        })
        .build(t)
        .key("pr-checkout-reference")
        .single_line();
    let mut body = col().w_full().gap(12.0).child(
        col().w_full().gap(6.0).child(text(capitalize(singular)).font(theme::XS_MEDIUM).color(t.c.foreground)).child(input),
    );
    if let Some(pr) = pr {
        let state = data::s(pr, "state");
        body = body.child(
            row()
                .key("pr-checkout-card")
                .w_full()
                .gap(12.0)
                .padding(12.0)
                .radius(radius::XL)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.muted.alpha_mul(0.24))
                .child(
                    col()
                        .grow(1.0)
                        .shrink()
                        .min_w(0.0)
                        .child(text(data::s(pr, "title").to_string()).font(theme::SM_MEDIUM).color(t.c.foreground).single_line().truncate())
                        .child(
                            text(format!("#{} · {} to {}", pr["number"], data::s(pr, "headBranch"), data::s(pr, "baseBranch")))
                                .font(theme::TEXT_XS)
                                .color(muted)
                                .single_line()
                                .truncate(),
                        ),
                )
                .child(text(capitalize(state)).font(theme::TEXT_XS).color(state_color(t, state))),
        );
    }
    if busy {
        body = body.child(
            row().gap(8.0).center_y().child(kit::Spinner::new().size("sm").build(&k)).child(text(format!("Resolving {singular}...")).font(theme::TEXT_XS).color(muted)),
        );
    }
    let validation = match (c.dirty, c.reference.text.trim().is_empty(), parsed.is_some()) {
        (false, ..) => None,
        (true, true, _) => Some(format!("Paste a {singular} URL, checkout command, or enter 123 / #123.")),
        (true, false, false) => Some(format!("Use a {singular} URL, checkout command, 123, or #123.")),
        _ => None,
    };
    let resolve_error = c.resolve_error.as_ref().filter(|e| Some(&e.0) == parsed.as_ref() && pr.is_none()).map(|e| e.1.clone());
    if let Some(error) = validation.or_else(|| c.error.clone()).or(resolve_error) {
        body = body.child(text(error).font(theme::TEXT_XS).color(t.c.destructive).key("pr-checkout-error"));
    }
    let pending = c.preparing.is_some();
    let ready = pr.is_some() && !busy && !pending && super::cwd(s).is_some();
    let label = |mode: Mode, idle: &str, busy: &str| if c.preparing.map(|p| p.1) == Some(mode) { busy.to_string() } else { idle.to_string() };
    let footer = row()
        .w_full()
        .gap(8.0)
        .child(el().grow(1.0))
        .child(kit::Button::new("Cancel").variant("outline").size("sm").disabled(pending).on_press(close).build(t))
        .child(
            kit::Button::new(label(Mode::Local, "Local", "Preparing local..."))
                .variant("outline")
                .size("sm")
                .disabled(!ready)
                .on_press(|s: &mut App| confirm(s, Mode::Local))
                .build(t),
        )
        .child(
            kit::Button::new(label(Mode::Worktree, "Worktree", "Preparing worktree..."))
                .size("sm")
                .disabled(!ready)
                .on_press(|s: &mut App| confirm(s, Mode::Worktree))
                .build(t),
        );
    kit::Dialog::new("pr-checkout", format!("Checkout {singular}"))
        .open(true)
        .width(576.0)
        .description(format!("Resolve a {} {singular}, then create the draft thread in the main repo or in a dedicated worktree.", provider_name(s)))
        .child(body)
        .child(footer)
        .on_close(close)
        .build(t, out);
}

fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    chars.next().map(|c| c.to_uppercase().chain(chars).collect()).unwrap_or_default()
}

/// ThreadDetailsPrRows: the branch's pull request, then the thread's other linked ones behind
/// "Show N more".
pub fn rows(s: &App, t: &T) -> Option<El> {
    let current = git::status(s).map(|v| v["pr"].clone()).filter(|v| v.is_object());
    let thread = super::composer::ctx(s).thread;
    let linked: Vec<Value> = thread
        .and_then(|t| t["pullRequests"].as_array())
        .into_iter()
        .flatten()
        .map(|l| {
            let mut v = l["snapshot"].clone();
            if !v.is_object() {
                v = json!({});
            }
            v["number"] = l["number"].clone();
            v["url"] = l["url"].clone();
            v
        })
        .collect();
    let first = current.clone().or_else(|| linked.first().cloned())?;
    let others: Vec<&Value> = linked.iter().filter(|l| l["number"] != first["number"]).collect();
    let expanded = s.ui.chat.details.pr_expanded;
    let mut out = col().key("details-prs").w_full().child(pr_row(t, &first, "details-pr-0"));
    if expanded {
        for (i, pr) in others.iter().enumerate() {
            out = out.child(pr_row(t, pr, &format!("details-pr-{}", i + 1)));
        }
    }
    if !others.is_empty() {
        let label = if expanded { "Show less".to_string() } else { format!("Show {} more", others.len()) };
        let chevron = icon(if expanded { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 16.0).color(t.c.muted_foreground);
        out = out.child(
            super::action(t, "details-pr-more", chevron, &label, t.c.muted_foreground)
                .w_full()
                .role("button")
                .expanded(expanded)
                .on_press(|s: &mut App| s.ui.chat.details.pr_expanded = !s.ui.chat.details.pr_expanded),
        );
    }
    Some(out)
}

fn pr_row(t: &T, pr: &Value, key: &str) -> El {
    let state = if pr["isDraft"] == true && data::s(pr, "state") == "open" { "draft" } else { data::s(pr, "state") };
    let color = if state == "draft" { t.c.muted_foreground } else { state_color(t, state) };
    let glyph = match state {
        "merged" => &icons::GIT_MERGE,
        "closed" => &icons::GIT_PULL_REQUEST_CLOSED,
        "draft" => &icons::GIT_PULL_REQUEST_DRAFT,
        _ => &icons::GIT_PULL_REQUEST_ARROW,
    };
    let title = data::s(pr, "title");
    let label = if title.is_empty() { format!("#{}", pr["number"]) } else { format!("#{}: {title}", pr["number"]) };
    let url = data::s(pr, "url").to_string();
    let head = data::s(pr, "headRef").to_string();
    let base = data::s(pr, "baseRef").to_string();
    let detail = if head.is_empty() { capitalize(state) } else { format!("{} · {head} into {base}", capitalize(state)) };
    row()
        .key(key.to_string())
        .w_full()
        .h(32.0)
        .px(10.0)
        .gap(9.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .cursor(Cursor::Pointer)
        .hover(t.c.accent)
        .role("link")
        .label(label.clone())
        .help(detail)
        .child(icon(glyph, 16.0).color(color))
        .child(text(label).font(theme::TEXT_SM).color(t.c.foreground.alpha_mul(0.8)).single_line().truncate().grow(1.0).shrink().min_w(0.0))
        .on_press(move |s: &mut App| {
            if !url.is_empty() {
                crate::settings::auth::open_url(s, &url);
            }
        })
}

#[cfg(test)]
mod tests {
    use super::parse_reference;

    #[test]
    fn pull_request_references_parse_like_web() {
        let ok = |input: &str, want: &str| assert_eq!(parse_reference(input).as_deref(), Some(want), "{input}");
        ok("42", "42");
        ok(" #42 ", "42");
        ok("gh pr checkout 42", "42");
        ok("glab mr checkout 7", "7");
        ok("tea pulls checkout 3", "3");
        ok("az repos pr checkout --id 9", "9");
        ok("gh pr checkout https://github.com/o/r/pull/5", "https://github.com/o/r/pull/5");
        ok("https://github.com/o/r/pull/5/files", "https://github.com/o/r/pull/5/files");
        ok("https://gitlab.com/g/p/-/merge_requests/12", "https://gitlab.com/g/p/-/merge_requests/12");
        ok("https://codeberg.org/o/r/pulls/8", "https://codeberg.org/o/r/pulls/8");
        ok("https://dev.azure.com/o/p/_git/r/pullrequest/4", "https://dev.azure.com/o/p/_git/r/pullrequest/4");
        for no in ["", "#", "feature/x", "main", "https://github.com/o/r", "https://github.com/o/r/pull/x", "gh pr checkout"] {
            assert_eq!(parse_reference(no), None, "{no}");
        }
    }
}
