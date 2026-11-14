//! Thread actions and the file picker, as CommandPalette's root actions run them.
use super::{Action, Item, item, keywords, shortcut_label, toast};
use crate::{
    App, Route, data, icons, kit,
    theme::{self, T},
};
use rune::{El, TextEdit, Tx, col, row, text};
use serde_json::{Value, json};

#[derive(Default)]
pub struct State {
    pub link: Option<LinkDialog>,
    pub files: Files,
    triage: Option<Triage>,
    scratch: Option<Scratch>,
    /// The server's keybindings and config issues last seen, to toast when a reload changes them.
    keybindings: Option<(Value, Value)>,
}
/// `openScratch`: the request, then the project id until the project reaches the store, since
/// a draft sends to the project it resolves.
enum Scratch {
    Request(u64),
    Await(String, i64),
}
pub struct LinkDialog {
    pub thread: String,
    pub input: TextEdit,
    pub dirty: bool,
    pub error: Option<String>,
    pub(super) pending: Option<u64>,
}
#[derive(Default)]
pub struct Files {
    pub(super) sent: Option<String>,
    pub(super) due: Option<i64>,
    pub(super) pending: Option<(u64, String)>,
    pub entries: Vec<String>,
    pub loaded: Option<String>,
    pub error: Option<String>,
}
/// Each step carries the environment the triage started on, which answers every follow-up.
enum Triage {
    Prepare(u64, String),
    Scratch(u64, String, String),
}

fn config(s: &App) -> &Value {
    static NULL: Value = Value::Null;
    s.state().config.as_ref().unwrap_or(&NULL)
}
fn thread(s: &App) -> Option<&Value> {
    let Route::Thread(id) = &s.route else { return None };
    s.state().threads.iter().find(|v| v["id"] == *id)
}
fn visible_pull_requests(thread: &Value) -> Vec<&Value> {
    thread["pullRequests"].as_array().into_iter().flatten().filter(|v| v["source"] != "stack-dismissed").collect()
}
/// The pull request a thread is about: its only open link, else the most recently updated one.
fn current_pull_request(thread: &Value) -> Option<String> {
    let visible = visible_pull_requests(thread);
    let open: Vec<&&Value> = visible.iter().filter(|v| v["snapshot"].is_null() || v["snapshot"]["state"] == "open").collect();
    let chosen = if open.len() == 1 {
        Some(*open[0])
    } else {
        visible.iter().copied().max_by_key(|v| {
            v["snapshot"]["updatedAt"].as_str().or(v["linkedAt"].as_str()).unwrap_or("").to_owned()
        })
    };
    chosen
        .and_then(|v| v["url"].as_str())
        .or(thread["linkedPullRequest"]["url"].as_str())
        .or(thread["branchPullRequest"]["url"].as_str())
        .map(str::to_owned)
}
fn hosted_segment(id: &str) -> String {
    let mut out = String::new();
    for b in id.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
/// `hostedThreadLink`: opens the thread from anywhere.
/// `hostedThreadLink` for a thread on this server.
pub fn thread_link(s: &App, thread: &str) -> String {
    hosted_thread_link(data::s(&config(s)["environment"], "environmentId"), thread)
}
fn hosted_thread_link(environment: &str, thread: &str) -> String {
    format!("https://app.s5code.touchtech.club/threads/{}/{}", hosted_segment(environment), hosted_segment(thread))
}

/// Root actions between "New thread without a project" and "Restart agent session".
pub fn thread_items(s: &App) -> Vec<Item> {
    let mut out = vec![];
    let capabilities = &config(s)["environment"]["capabilities"];
    if capabilities["triage"] == true {
        let mut v = item("Triage an issue", "Actions", &icons::BUG, "", Action::Triage);
        v.description = Some("An agent investigates an S5 Code problem and helps file an issue".into());
        v.fields = keywords(&["triage", "bug", "issue", "report", "broken", "debug", "help", "support"]);
        out.push(v);
    }
    let current = thread(s);
    if current.is_some() && config(s)["threadFind"] == true {
        let mut v = item("Find in current thread", "Actions", &icons::TEXT_SEARCH, "", Action::Find);
        v.shortcut = shortcut_label(s, "chat.find");
        v.fields = keywords(&["find", "search", "messages", "plans"]);
        out.push(v);
    }
    out.extend(prompt_link_items(s));
    let Some(thread) = current else { return out };
    let pr = current_pull_request(thread);
    let mut copy = item(if pr.is_some() { "Copy PR link" } else { "Copy thread" }, "Actions", &icons::LINK, "", Action::CopyThread);
    copy.description = Some(pr.unwrap_or_else(|| data::s(thread, "title").to_owned()));
    copy.shortcut = shortcut_label(s, "thread.copyReference");
    copy.fields = keywords(&["copy", "link", "share", "thread", "pull request", "pr"]);
    out.push(copy);
    if capabilities.is_object() {
        let mut link = item("Link pull request to thread", "Actions", &icons::LINK_2, "", Action::LinkPr(data::s(thread, "id").to_owned()));
        link.fields = keywords(&["link", "pull request", "pr", "attach", "stack"]);
        out.push(link);
        let mut show = item("Show linked pull requests", "Actions", &icons::LINK_2, "", Action::ShowPrs);
        show.disabled = visible_pull_requests(thread).is_empty();
        show.fields = keywords(&["pull requests", "linked", "stack", "prs"]);
        out.push(show);
    }
    if crate::shell::sidebar::share::can_share_with_guest(s) {
        let mut share = item("Share with guest", "Actions", &icons::USER_PLUS, "", Action::ShareWithGuest(data::s(thread, "id").to_owned()));
        share.fields = keywords(&["share", "guest", "invite", "link", "teammate"]);
        out.push(share);
    }
    out
}

/// `composerPromptLink`: the open draft's prompt as a new-thread link, and for a GitHub project
/// the same link with its repository and the draft's provider and model.
fn prompt_link_items(s: &App) -> Vec<Item> {
    let route = s.frame_route();
    if !matches!(route, Route::Thread(_) | Route::Home { .. }) {
        return vec![];
    }
    let Some(draft) = s.ui.chat.composer.drafts.get(&crate::chat::composer::draft_key(s, route)) else { return vec![] };
    let Some(plain) = crate::links::new_thread_link(&draft.edit.text, None, None, None) else { return vec![] };
    let mut copy = item("Copy prompt link", "Actions", &icons::LINK, "", Action::CopyPromptLink(plain));
    copy.fields = keywords(&["copy", "share", "prompt", "link"]);
    let mut out = vec![copy];
    let ctx = crate::chat::composer::ctx(s);
    if let Some(repo) = ctx.project.and_then(crate::links::repository_of) {
        let thread_model = ctx.thread.map(|t| &t["modelSelection"]).filter(|m| !data::s(m, "instanceId").is_empty()).map(|m| (data::s(m, "instanceId").to_owned(), data::s(m, "model").to_owned()));
        let selection = draft.model.clone().or(thread_model);
        let provider = selection.as_ref().and_then(|(instance, _)| crate::chat::composer::provider(s, instance)).map(|p| data::s(p, "driver").to_owned());
        let model = selection.as_ref().map(|(_, model)| model.as_str());
        if let Some(link) = crate::links::new_thread_link(&draft.edit.text, Some(&repo), provider.as_deref(), model) {
            let mut v = item("Copy prompt link with repo and model", "Actions", &icons::LINK, "", Action::CopyPromptLink(link));
            v.fields = keywords(&["copy", "share", "prompt", "link", "repository", "model"]);
            out.push(v);
        }
    }
    out
}
/// Root actions after "Restart agent session".
pub fn tail_items(s: &App) -> Vec<Item> {
    let mut out = vec![];
    let mut files = item("Go to file", "Actions", &icons::FILE_SEARCH, "", Action::Files);
    files.shortcut = shortcut_label(s, "filePicker.toggle");
    files.fields = keywords(&["go to file", "open file", "file picker", "find file", "quick open"]);
    out.push(files);
    let mut contents = item("Search project contents", "Actions", &icons::TEXT_SEARCH, "", Action::ContentSearch);
    contents.shortcut = shortcut_label(s, "projectSearch.toggle");
    contents.fields = keywords(&["search project", "find in files", "grep", "content search", "text search"]);
    out.push(contents);
    if super::add_project::new_projects_root(s).is_some() {
        let mut new = item("New project", "Actions", &icons::FOLDER_GIT_2, "", Action::NewProject);
        new.fields = keywords(&["new project", "create project", "empty", "repository", "repo", "git init"]);
        out.push(new);
    }
    let mut add = item("Add project", "Actions", &icons::FOLDER_PLUS, "", Action::AddProject);
    add.fields = keywords(&[
        "add project", "folder", "directory", "browse", "clone", "remote", "repository", "repo", "git", "github", "gitlab",
        "gitcafe", "forgejo", "gitea", "bitbucket", "azure", "devops", "url", "environment",
    ]);
    out.push(add);
    // Electron's App and Help menu item, which Rune has as a native menu only on macOS.
    let mut updates = item("Check for Updates", "Actions", &icons::DOWNLOAD, "", Action::CheckForUpdates);
    updates.fields = keywords(&["check for updates", "update", "upgrade", "new version", "release"]);
    out.push(updates);
    out
}

/// `availableScratchWorkspaceRoot`: the server can host threads without a project.
pub fn scratch_available(s: &App) -> bool {
    config(s)["scratchWorkspaceRoot"].as_str().is_some_and(|root| !root.is_empty())
}

/// `isScratchProject`: the project rooted at the server's scratch workspace.
pub fn is_scratch_project(s: &App, project: &Value) -> bool {
    let root = |p: &str| p.trim_end_matches(['/', '\\']).to_owned();
    let scratch = data::s(config(s), "scratchWorkspaceRoot");
    !scratch.is_empty() && root(data::s(project, "workspaceRoot")) == root(scratch)
}

/// `startScratchThread`: opens the server's Scratch project, then a new thread draft in it.
pub fn new_without_project(s: &mut App) {
    if s.ui.shell.palette.act.scratch.is_none()
        && let Some(net) = s.src.net()
    {
        s.ui.shell.palette.act.scratch = Some(Scratch::Request(net.request("projects.ensureScratch", json!({}))));
    }
}

/// A scratch thread is still starting.
pub fn scratch_pending(s: &App) -> bool {
    s.ui.shell.palette.act.scratch.is_some()
}

pub fn triage(s: &mut App) {
    if s.ui.shell.palette.act.triage.is_some() {
        return;
    }
    if config(s)["scratchWorkspaceRoot"].as_str().is_none_or(str::is_empty) {
        return triage_failed(s, "This machine cannot start threads without a project.");
    }
    let client = json!({"deviceType": "desktop", "surface": "desktop", "os": std::env::consts::OS});
    if let Some(net) = s.src.net() {
        let id = net.request("server.prepareTriage", json!({"client": client}));
        s.ui.shell.palette.act.triage = Some(Triage::Prepare(id, s.ui.env.active.clone()));
    }
}
fn triage_failed(s: &mut App, message: &str) {
    toast(s, "Could not start triage", Some(&format!("{message} Run npx t3 triage in a terminal on that machine instead.")));
}
/// `chat.find`: the thread's find bar.
pub fn find(s: &mut App) {
    crate::chat::timeline::find::open(s);
}
/// Older servers drop project pin and archive (`canOrganizeProjectGroup`).
pub fn can_organize_projects(s: &App) -> bool {
    config(s)["environment"]["capabilities"]["projectOrganization"] == true
}
/// `toggleProjectGroupPin`: `project.update` with the new pin state.
pub fn pin_project(s: &mut App, project: &str, pinned: bool) {
    if let Some(net) = crate::env::net_for(s, project) {
        net.request("projects.mutate", json!({"type": "project.update", "commandId": s5_net::uuid(), "projectId": project, "pinned": pinned}));
    }
}
pub fn copy_thread(s: &mut App, tx: &mut Tx) -> bool {
    let Some(thread) = thread(s) else { return false };
    let (value, title) = match current_pull_request(thread) {
        Some(url) => (url.clone(), "PR link copied"),
        None => {
            let environment = data::s(&config(s)["environment"], "environmentId");
            (hosted_thread_link(environment, data::s(thread, "id")), "Thread copied")
        }
    };
    let description = if title == "Thread copied" { data::s(thread, "title").to_owned() } else { value.clone() };
    tx.copy(value);
    toast(s, title, Some(&description));
    true
}
/// Rune's right panel has no pull-requests surface; opening the panel is the nearest place.
pub fn show_pull_requests(s: &mut App) {
    s.ui.shell.panel_open = true;
}
pub fn open_link(s: &mut App, thread: String) {
    let mut input = TextEdit::default();
    input.focused = true;
    s.ui.shell.palette.act.link = Some(LinkDialog { thread, input, dirty: false, error: None, pending: None });
}

// Pull request references, as the source control definitions parse them.

#[derive(Debug)]
pub struct Link {
    pub host: String,
    pub repository: String,
    pub number: u64,
    pub url: String,
}
fn number(v: &str) -> Option<u64> {
    (!v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())).then(|| v.parse().ok()).flatten().filter(|n| *n > 0)
}
pub fn parse_url(input: &str) -> Option<Link> {
    let rest = input.strip_prefix("https://").or_else(|| input.strip_prefix("http://"))?;
    let rest = rest.split(['?', '#']).next()?;
    let (host, path) = rest.split_once('/')?;
    let host = host.rsplit('@').next()?.to_lowercase();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let n = segments.len();
    let found = |repo: &[&str], num: &str| Some(Link { host: host.clone(), repository: repo.join("/"), number: number(num)?, url: input.into() });
    if n == 4 && segments[2] == "pull" && host != "bitbucket.org" {
        return found(&segments[..2], segments[3]);
    }
    if n >= 5 && segments[n - 3] == "-" && segments[n - 2] == "merge_requests" {
        return found(&segments[..n - 3], segments[n - 1]);
    }
    if n == 4 && segments[2] == "pull-requests" {
        return found(&segments[..2], segments[3]);
    }
    if n >= 4 && segments[n - 2] == "pullrequest" && segments[n - 4] == "_git" {
        let mut repo = segments[..n - 4].to_vec();
        repo.push(segments[n - 3]);
        return found(&repo, segments[n - 1]);
    }
    if host.ends_with("git.cafe") && n == 4 && segments[2] == "pull" {
        return found(&segments[..2], segments[3]);
    }
    if n >= 4 && segments[n - 2] == "pulls" {
        return found(&segments[..n - 2], segments[n - 1]);
    }
    None
}
/// The thread's repository, so a bare number resolves to one of its pull requests.
fn project_link(identity: &Value, number: u64) -> Result<Link, String> {
    let repository = identity["displayName"].as_str().map(str::to_owned).or_else(|| {
        Some(format!("{}/{}", identity["owner"].as_str()?, identity["name"].as_str()?))
    });
    let Some(repository) = repository.filter(|_| identity.is_object()) else {
        return Err("Paste the full pull request URL; this thread has no repository to resolve the number against.".into());
    };
    let provider = identity["provider"].as_str().unwrap_or("");
    let (singular, suffix) = match provider {
        "github" => ("pull request", format!("/pull/{number}")),
        "gitlab" => ("merge request", format!("/-/merge_requests/{number}")),
        "gitcafe" | "forgejo" => ("pull request", format!("/pulls/{number}")),
        "bitbucket" => ("pull request", format!("/pull-requests/{number}")),
        "azure-devops" => ("pull request", format!("/pullrequest/{number}")),
        _ => ("pull request", String::new()),
    };
    let base = identity["webUrl"].as_str().map(|v| v.trim_end_matches('/').to_owned());
    let url = base.filter(|_| !suffix.is_empty()).map(|base| format!("{base}{suffix}"));
    url.and_then(|url| parse_url(&url))
        .map(|mut link| {
            link.repository = if link.repository.is_empty() { repository } else { link.repository };
            link
        })
        .ok_or_else(|| format!("Paste a full URL; this project's host has no known {singular} URL."))
}
/// `resolveLinkPullRequestInput`; `None` when the text is neither a URL nor a number.
pub fn resolve(reference: &str, identity: &Value) -> Option<Result<Link, String>> {
    let reference = reference.trim();
    if let Some(link) = parse_url(reference) {
        return Some(Ok(link));
    }
    let n = number(reference.strip_prefix('#').unwrap_or(reference))?;
    Some(project_link(identity, n))
}
fn link_identity(s: &App, thread: &str) -> Value {
    let project = s.state().threads.iter().find(|v| v["id"] == thread).map(|v| v["projectId"].clone());
    s.state().projects.iter().find(|p| Some(&p["id"]) == project.as_ref()).map(|p| p["repositoryIdentity"].clone()).unwrap_or(Value::Null)
}
fn validation(s: &App, dialog: &LinkDialog) -> (Option<Link>, Option<String>) {
    let resolved = resolve(&dialog.input.text, &link_identity(s, &dialog.thread));
    let (link, error) = match resolved {
        Some(Ok(link)) => (Some(link), None),
        Some(Err(e)) => (None, Some(e)),
        None if dialog.input.text.trim().is_empty() => (None, Some("Paste a pull request URL or enter 123 / #123.".into())),
        None => (None, Some("Use a pull request URL, 123, or #123.".into())),
    };
    (link, if dialog.dirty { error } else { None })
}
fn submit_link(s: &mut App) {
    let Some(dialog) = &s.ui.shell.palette.act.link else { return };
    if dialog.pending.is_some() {
        return;
    }
    let (link, _) = validation(s, dialog);
    let thread = dialog.thread.clone();
    let dialog = s.ui.shell.palette.act.link.as_mut().unwrap();
    dialog.dirty = true;
    let Some(link) = link else { return };
    dialog.error = None;
    let command = json!({
        "type": "thread.pull-request.link",
        "threadId": thread,
        "host": link.host.to_lowercase(),
        "repository": link.repository.to_lowercase(),
        "number": link.number,
        "url": link.url,
        "source": "manual",
    });
    let pending = crate::env::net_for(s, &thread).map(|net| net.dispatch(command));
    if let Some(dialog) = &mut s.ui.shell.palette.act.link {
        dialog.pending = pending;
    }
}
fn close_link(s: &mut App) {
    if s.ui.shell.palette.act.link.as_ref().is_some_and(|d| d.pending.is_none()) {
        s.ui.shell.palette.act.link = None;
    }
}
pub fn link_dialog(s: &App, t: &T, layer: &mut Vec<El>) {
    let Some(dialog) = &s.ui.shell.palette.act.link else { return };
    let (link, validation) = validation(s, dialog);
    let pending = dialog.pending.is_some();
    let input = kit::Input::new(&dialog.input)
        .placeholder("Pull request URL or #42")
        .on_edit(|s: &mut App| &mut s.ui.shell.palette.act.link.as_mut().unwrap().input)
        .on_change(|s: &mut App, _| {
            if let Some(d) = &mut s.ui.shell.palette.act.link {
                d.dirty = true;
            }
        })
        .on_submit(|s: &mut App, _| submit_link(s))
        .build(t);
    let mut body = col().w_full().gap(8.).child(input);
    if let Some(link) = &link {
        body = body.child(
            text(format!("{}/{} #{}", link.host, link.repository, link.number))
                .font(theme::TEXT_XS)
                .color(t.c.contrast_muted_foreground)
                .truncate(),
        );
    }
    if let Some(error) = validation.or(dialog.error.clone()) {
        body = body.child(text(error).font(theme::TEXT_XS).color(t.c.destructive));
    }
    let cancel = kit::Button::new("Cancel").variant("outline").size("sm").disabled(pending).on_press(close_link).build(t);
    let confirm = kit::Button::new(if pending { "Linking..." } else { "Link" })
        .size("sm")
        .disabled(pending || link.is_none())
        .on_press(submit_link)
        .build(t);
    kit::Dialog::new("palette-link-pr", "Link pull request")
        .open(true)
        .width(576.)
        .description("Attach a pull request to this thread by its URL, or by its number for this thread's repository.")
        .child(body)
        .child(row().w_full().gap(8.).justify_end().child(cancel).child(confirm))
        .on_close(close_link)
        .build(t, layer);
}

// File picker.

fn files_context(s: &App) -> Option<(String, String)> {
    let ctx = crate::chat::composer::ctx(s);
    let project = ctx.project?;
    let cwd = ctx.thread.and_then(|t| t["worktreePath"].as_str()).filter(|v| !v.is_empty()).unwrap_or(data::s(project, "workspaceRoot"));
    Some((data::s(project, "title").to_owned(), cwd.to_owned()))
}
/// Debounces the search 120 ms after the query settles, as the web picker does.
pub fn files_changed(s: &mut App, tx: &mut Tx) {
    if s.ui.shell.palette.page != "files" {
        return;
    }
    s.ui.shell.palette.act.files.due = Some(data::now_ms() + 120);
    tx.wake_after(std::time::Duration::from_millis(120));
}
pub fn open_files(s: &mut App) {
    s.ui.shell.palette.act.files = Files::default();
    s.ui.shell.palette.act.files.due = Some(0);
}
fn subsequence(text: &str, query: &str) -> Option<Vec<(usize, usize)>> {
    let mut ranges: Vec<(usize, usize)> = vec![];
    let mut needle = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).peekable();
    needle.peek()?;
    for (i, c) in text.char_indices() {
        let Some(want) = needle.peek() else { break };
        if c.to_lowercase().eq(std::iter::once(*want)) {
            needle.next();
            match ranges.last_mut() {
                Some(last) if last.1 == i => last.1 = i + c.len_utf8(),
                _ => ranges.push((i, i + c.len_utf8())),
            }
        }
    }
    needle.peek().is_none().then_some(ranges)
}
pub fn file_items(s: &App) -> Vec<Item> {
    let files = &s.ui.shell.palette.act.files;
    if files_context(s).is_none() {
        return vec![];
    }
    let query = s.ui.shell.palette.query.text.trim();
    files.entries.iter().map(|path| {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned();
        let mut v = item(name.clone(), FILES_GROUP, &icons::FILE, "", Action::OpenFile(path.clone()));
        v.description = Some(path.clone());
        if !query.is_empty() {
            v.highlight = Some((subsequence(&name, query).unwrap_or_default(), subsequence(path, query).unwrap_or_default()));
        }
        v
    }).collect()
}
/// File rows group under the project's title, which `heading` supplies at render time.
pub const FILES_GROUP: &str = "files";
pub fn heading(s: &App) -> String {
    files_context(s).map(|(title, _)| title).unwrap_or_default()
}
pub fn files_empty(s: &App) -> &'static str {
    let files = &s.ui.shell.palette.act.files;
    if files_context(s).is_none() {
        "Open a project to search its files."
    } else if files.loaded.is_none() {
        if files.sent.as_deref().unwrap_or("").is_empty() { "Indexing workspace files…" } else { "Searching workspace files…" }
    } else if s.ui.shell.palette.query.text.trim().is_empty() {
        "No files found."
    } else {
        "No matching files."
    }
}
pub fn highlighted(content: &str, ranges: &[(usize, usize)], font: rune::tok::TextStyle, base: rune::Color, strong: rune::Color) -> El {
    let spans = ranges.iter().map(|(a, b)| rune::Span::new(*a, *b).weight(theme::SEMIBOLD).color(strong)).collect();
    rune::rich(content, spans).font(font).color(base).truncate()
}

/// KeybindingsUpdateToast: a reload that changes the bindings or their issues, after the first.
fn keybindings_reload(s: &mut App) -> Option<(&'static str, String)> {
    let config = s.state().config.as_ref()?;
    let (bindings, issues) = (&config["keybindings"], &config["issues"]);
    let seen = &s.ui.shell.palette.act.keybindings;
    if seen.as_ref().is_some_and(|(b, i)| b == bindings && i == issues) {
        return None;
    }
    let first = seen.is_none();
    let (bindings, issues) = (bindings.clone(), issues.clone());
    s.ui.shell.palette.act.keybindings = Some((bindings, issues.clone()));
    if first {
        return None;
    }
    let issue = issues.as_array().into_iter().flatten().find(|v| v["kind"].as_str().is_some_and(|k| k.starts_with("keybindings.")));
    Some(match issue {
        Some(issue) => ("Invalid keybindings configuration", data::s(issue, "message").to_owned()),
        None => ("Keybindings updated", "Keybindings configuration reloaded successfully.".to_owned()),
    })
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let files = &s.ui.shell.palette.act.files;
    if s.ui.shell.palette.page == "files" && files.due.is_some_and(|due| data::now_ms() >= due) {
        s.ui.shell.palette.act.files.due = None;
        let query = s.ui.shell.palette.query.text.trim().to_owned();
        if let Some((_, cwd)) = files_context(s)
            && s.ui.shell.palette.act.files.sent.as_ref() != Some(&query)
            && let Some(net) = s.src.net()
        {
            let id = net.request("projects.searchEntries", json!({"cwd": cwd, "query": query, "limit": 200, "kind": "file"}));
            s.ui.shell.palette.act.files.pending = Some((id, query.clone()));
            s.ui.shell.palette.act.files.sent = Some(query);
        }
    }
    if let Some((id, query)) = s.ui.shell.palette.act.files.pending.clone()
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        let files = &mut s.ui.shell.palette.act.files;
        files.pending = None;
        match reply {
            Ok(v) => {
                files.entries = v["entries"].as_array().into_iter().flatten()
                    .filter(|e| e["kind"] != "directory")
                    .map(|e| data::s(e, "path").to_owned())
                    .collect();
                files.error = None;
            }
            Err(e) => files.error = Some(super::error_message(&e, "Could not search files.")),
        }
        files.loaded = Some(query);
        changed = true;
    }
    if let Some(dialog) = &s.ui.shell.palette.act.link
        && let Some(id) = dialog.pending
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        match reply {
            Ok(_) => s.ui.shell.palette.act.link = None,
            Err(e) => {
                let dialog = s.ui.shell.palette.act.link.as_mut().unwrap();
                dialog.pending = None;
                dialog.error = Some(super::error_message(&e, "Could not link the pull request."));
            }
        }
        changed = true;
    }
    if let Some((title, description)) = keybindings_reload(s) {
        toast(s, title, Some(&description));
        changed = true;
    }
    const SCRATCH_FAILED: &str = "Could not start without a project";
    match s.ui.shell.palette.act.scratch.take() {
        Some(Scratch::Request(id)) => match s.src.net().and_then(|net| net.take_reply(id)) {
            None => s.ui.shell.palette.act.scratch = Some(Scratch::Request(id)),
            Some(Ok(v)) => s.ui.shell.palette.act.scratch = Some(Scratch::Await(data::s(&v, "projectId").to_owned(), data::now_ms() + 10_000)),
            Some(Err(e)) => {
                toast(s, SCRATCH_FAILED, Some(&super::error_message(&e, "An error occurred.")));
                changed = true;
            }
        },
        Some(Scratch::Await(id, _)) if crate::chat::composer::project_by_id(s, &id).is_some() => {
            crate::shell::sidebar::go(s, Route::Home { project_id: Some(id) });
            changed = true;
        }
        Some(Scratch::Await(_, deadline)) if data::now_ms() >= deadline => {
            toast(s, SCRATCH_FAILED, Some("The scratch project did not load."));
            changed = true;
        }
        pending => s.ui.shell.palette.act.scratch = pending,
    }
    match s.ui.shell.palette.act.triage.take() {
        Some(Triage::Prepare(id, env)) => match s.src.net().and_then(|net| net.take_reply(id)) {
            None => s.ui.shell.palette.act.triage = Some(Triage::Prepare(id, env)),
            Some(Err(e)) => {
                triage_failed(s, &super::error_message(&e, "An error occurred."));
                changed = true;
            }
            Some(Ok(v)) => {
                let prompt = data::s(&v, "prompt").to_owned();
                if let Some(net) = crate::env::net_of(s, &env) {
                    s.ui.shell.palette.act.triage = Some(Triage::Scratch(net.request("projects.ensureScratch", json!({})), prompt, env));
                }
            }
        },
        Some(Triage::Scratch(id, prompt, env)) => match s.src.net().and_then(|net| net.take_reply(id)) {
            None => s.ui.shell.palette.act.triage = Some(Triage::Scratch(id, prompt, env)),
            Some(Err(e)) => {
                triage_failed(s, &super::error_message(&e, "An error occurred."));
                changed = true;
            }
            Some(Ok(v)) => {
                let project = data::s(&v, "projectId").to_owned();
                crate::env::adopt(s, &project, &env);
                let route = Route::Home { project_id: Some(project) };
                let key = crate::chat::composer::draft_key(s, &route);
                s.ui.chat.composer.drafts.entry(key).or_default().edit = TextEdit::new(prompt);
                crate::shell::sidebar::go(s, route);
                changed = true;
            }
        },
        None => {}
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keybinding_reloads_toast_after_the_first_snapshot() {
        let mut app = App::new();
        let set = |app: &mut App, bindings: Value, issues: Value| {
            if let crate::data::Source::Fixture(st) = &mut app.src {
                st.config = Some(json!({ "keybindings": bindings, "issues": issues }));
            }
            keybindings_reload(app)
        };
        assert_eq!(set(&mut app, json!([{ "command": "a" }]), json!([])), None, "the first snapshot is silent");
        assert_eq!(set(&mut app, json!([{ "command": "a" }]), json!([])), None, "nothing changed");
        assert_eq!(set(&mut app, json!([{ "command": "b" }]), json!([])).map(|t| t.0), Some("Keybindings updated"));
        let bad = json!([{ "kind": "keybindings.invalid-entry", "message": "bad key" }]);
        assert_eq!(set(&mut app, json!([{ "command": "b" }]), bad), Some(("Invalid keybindings configuration", "bad key".to_owned())));
    }
    #[test]
    fn pull_request_urls_and_numbers_resolve() {
        let gh = parse_url("https://github.com/Acme/Widgets/pull/42").unwrap();
        assert_eq!((gh.host.as_str(), gh.repository.as_str(), gh.number), ("github.com", "Acme/Widgets", 42));
        let gl = parse_url("https://gitlab.com/group/sub/proj/-/merge_requests/7").unwrap();
        assert_eq!((gl.repository.as_str(), gl.number), ("group/sub/proj", 7));
        assert_eq!(parse_url("https://bitbucket.org/ws/repo/pull-requests/3").unwrap().repository, "ws/repo");
        assert_eq!(parse_url("https://dev.azure.com/org/proj/_git/repo/pullrequest/9").unwrap().repository, "org/proj/repo");
        assert_eq!(parse_url("https://codeberg.org/a/b/pulls/5").unwrap().number, 5);
        assert!(parse_url("ftp://github.com/a/b/pull/1").is_none());
        assert!(parse_url("https://github.com/a/b/issues/1").is_none());
        let identity = json!({"provider":"github","owner":"acme","name":"widgets","webUrl":"https://github.com/acme/widgets"});
        let link = resolve("#12", &identity).unwrap().unwrap();
        assert_eq!(link.url, "https://github.com/acme/widgets/pull/12");
        assert!(resolve("12", &Value::Null).unwrap().unwrap_err().contains("no repository"));
        assert!(resolve("12", &json!({"owner":"a","name":"b"})).unwrap().unwrap_err().contains("no known pull request URL"));
        assert!(resolve("hello", &identity).is_none());
        assert!(resolve("#0", &identity).is_none());
    }
    #[test]
    fn hosted_links_escape_url_safe_punctuation() {
        assert_eq!(hosted_thread_link("env.1", "thread:a!b"), "https://app.s5code.touchtech.club/threads/env%2E1/thread%3Aa%21b");
        let thread = json!({"pullRequests":[
            {"url":"u1","source":"manual","linkedAt":"2026-01-01","snapshot":{"state":"merged","updatedAt":"2026-03-01"}},
            {"url":"u2","source":"manual","linkedAt":"2026-01-02","snapshot":{"state":"open","updatedAt":"2026-02-01"}},
            {"url":"u3","source":"stack-dismissed","linkedAt":"2026-05-02","snapshot":null}
        ]});
        assert_eq!(current_pull_request(&thread).as_deref(), Some("u2"));
        assert_eq!(subsequence("palette.rs", "plt"), Some(vec![(0, 1), (2, 3), (4, 5)]));
        assert_eq!(subsequence("mod.rs", "xyz"), None);
    }
}
