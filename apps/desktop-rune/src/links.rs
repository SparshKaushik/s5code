//! Prompt links (packages/shared/src/promptLinks.ts): `s5code://new?prompt=…&repo=owner/name`,
//! or the hosted app's `/new?…`, open a new-thread draft holding the prompt on the repository's
//! project (NewThreadLinkView): with no project for it, they offer to clone it; with no
//! repository, the draft opens in the Scratch project. `/new?pr=owner/name/123` opens the
//! thread already on that pull request, else offers to check it out. Nothing is sent; the
//! person reads the prompt in the composer first. They arrive on the command line (Linux and Windows hand links
//! to a new process) or through macOS's `on_open_url`, and wait for the shell before they land.
//!
//! Web pages the OS hands over (macOS: http links and `.html` files) open in a thread's browser,
//! as DesktopWebLinkCoordinator's `openInBrowser`.

use crate::settings::codex_callback;
use crate::{App, Route, data};

/// `PROMPT_LINK_MAX_PROMPT_LENGTH`.
const MAX_PROMPT: usize = 8_000;
/// `DEFAULT_HOSTED_APP_URL` (connectAuth.ts).
const HOSTED_APP: &str = "https://app.s5code.touchtech.club";
/// `PROMPT_LINK_REPOSITORY_HOSTS`: git.cafe mirrors GitHub under the same path.
const REPOSITORY_HOSTS: [&str; 3] = ["github.com", "git.cafe", "staging.git.cafe"];

#[derive(Debug, PartialEq)]
pub struct NewThread {
    /// Empty only on a pull-request link, whose prompt is optional.
    prompt: String,
    /// The pull request's repository on a pull-request link.
    repository: Option<(String, String)>,
    /// The provider driver the link asks for, lowercased, and its model.
    provider: Option<String>,
    model: Option<String>,
    pull_request: Option<u64>,
    /// Hidden characters dropped from the prompt, which web warns about.
    hidden: usize,
}

/// A thread deep link: the thread, and the prompt it carries for its composer.
#[derive(Debug, PartialEq)]
struct ThreadLink {
    thread: String,
    prompt: Option<Result<(String, usize), String>>,
}

#[derive(Default)]
pub struct Ui {
    pending: Option<Result<NewThread, String>>,
    pending_thread: Option<ThreadLink>,
    /// A pairing link waiting for the person to confirm its backend: (pairing URL, origin).
    pair: Option<(String, String)>,
    /// Pairing URLs already used; their one-time token is spent.
    paired: Vec<String>,
    /// A confirmed pairing link is pairing, to report how it went.
    pairing: bool,
    web: Web,
    landing: Option<Landing>,
}

/// NewThreadLinkView's states after a link is read and before its draft opens.
#[derive(Debug, PartialEq)]
enum Landing {
    /// "You don't have owner/name": no project has the link's repository.
    Missing(NewThread),
    /// `projectClone.start` sent to `env` for the new project `project`.
    Cloning { request: u64, env: String, project: String, link: NewThread },
    /// The clone started; `waitForProject` gives its project until `deadline` to appear.
    Awaiting { project: String, link: NewThread, deadline: std::time::Instant },
    /// A scratch project is starting for the link's draft.
    Scratch(NewThread),
    /// `pullRequests.linkedThreads` on each of the repository's projects, most recent first:
    /// (environment, request, reply). `project` is where a checkout would go.
    Lookup { requests: Vec<(String, u64, Option<s5_net::Reply>)>, link: NewThread, project: String },
    /// "Check out owner/name#N?": checkout runs the pull request's setup script, so a link alone
    /// never starts it.
    Checkout { link: NewThread, project: String, started: bool },
}

/// `waitForProject`'s 5 seconds.
const CLONE_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

const CANNOT_CLONE: &str = "This machine cannot clone from a link. Update T3 Code on it, or clone the repository from the command palette.";
const NO_SCRATCH: &str = "None of your machines can start a thread without a project right now.";

/// Electron's `isWebLink` or `isWebPageFile`: an http(s) URL, or the path of an HTML file.
#[derive(Debug, PartialEq, Clone)]
pub enum WebPage {
    Url(String),
    File(String),
}

/// A web page on its way to a browser tab.
#[derive(Debug, Default)]
enum Web {
    #[default]
    Idle,
    Pending(WebPage),
    /// No thread shows a browser, so a scratch thread is starting for it.
    Scratch(WebPage),
    /// The file's `assets.createUrl` request; this machine's server serves the page.
    Asset(u64, String),
}

/// `HIDDEN_CHARACTERS`: characters that let a model read instructions the person can't see.
fn hidden(c: char) -> bool {
    matches!(c as u32,
        0x00..=0x08 | 0x0B | 0x0C | 0x0E..=0x1F | 0x7F..=0x9F | 0xAD | 0x34F | 0x61C | 0x115F | 0x1160
        | 0x17B4 | 0x17B5 | 0x180B..=0x180F | 0x200B | 0x200C | 0x200E | 0x200F | 0x202A..=0x202E
        | 0x2060..=0x206F | 0x3164 | 0xFE00..=0xFE0D | 0xFEFF | 0xFFA0 | 0xFFF0..=0xFFFB
        | 0x1BCA0..=0x1BCA3 | 0x1D173..=0x1D17A | 0xE0000..=0xE0FFF)
}

fn emoji_part(c: Option<&char>) -> bool {
    c.is_some_and(|&c| matches!(c as u32, 0x1F000..=0x1FAFF | 0x2190..=0x2BFF | 0xFE0F))
}

/// `sanitizeLinkedPrompt`: the visible prompt and how many hidden characters it dropped.
fn sanitize(raw: &str) -> (String, usize) {
    let mut dropped = 0;
    let unified = raw.replace("\r\n", "\n").replace(['\r', '\u{2028}', '\u{2029}'], "\n");
    let visible: Vec<char> = unified.chars().filter(|&c| !(hidden(c) && { dropped += 1; true })).collect();
    let text: String = visible
        .iter()
        .enumerate()
        .filter(|&(i, &c)| {
            let kept = c != '\u{200D}' || (i > 0 && emoji_part(visible.get(i - 1)) && emoji_part(visible.get(i + 1)));
            if !kept {
                dropped += 1;
            }
            kept
        })
        .map(|(_, c)| c)
        .collect();
    (text.trim().to_owned(), dropped)
}

/// What follows `https://github.com/` (any case, http, `www.`), if `value` starts with it.
fn github_path(value: &str) -> Option<&str> {
    ["https://github.com/", "http://github.com/", "https://www.github.com/", "http://www.github.com/"]
        .iter()
        .find_map(|p| value.get(..p.len()).filter(|head| head.eq_ignore_ascii_case(p)).map(|_| &value[p.len()..]))
}

/// `parsePromptLinkRepository`: `owner/name` or a github.com repository URL.
fn repository(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    let path = github_path(value).map_or(value, |path| path.trim_end_matches('/'));
    let (owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").or_else(|| name.strip_suffix(".GIT")).unwrap_or(name);
    let owner_ok = (1..=39).contains(&owner.len()) && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !owner.starts_with('-');
    let name_ok = (1..=100).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !name.chars().all(|c| c == '.');
    (owner_ok && name_ok).then(|| (owner.to_owned(), name.to_owned()))
}

/// `parsePromptLinkPullRequest`: a github.com pull request URL, `owner/name/123` or
/// `owner/name#123`.
fn pull_request(value: &str) -> Option<((String, String), u64)> {
    let value = value.trim();
    let (owner, name, number) = match github_path(value) {
        Some(path) => {
            let mut parts = path.splitn(4, '/');
            let (owner, name, pull, rest) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
            if !pull.eq_ignore_ascii_case("pull") || [owner, name].iter().any(|p| p.contains(['?', '#'])) {
                return None;
            }
            (owner, name, &rest[..rest.find(['/', '?', '#']).unwrap_or(rest.len())])
        }
        None => {
            let (owner, rest) = value.split_once('/')?;
            let at = rest.find(['/', '#'])?;
            if [owner, &rest[..at]].iter().any(|p| p.contains(|c: char| c.is_whitespace() || c == '#')) {
                return None;
            }
            (owner, &rest[..at], &rest[at + 1..])
        }
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    // `Number.isSafeInteger`.
    let number = number.parse::<u64>().ok().filter(|n| (1..1 << 53).contains(n))?;
    Some((repository(&format!("{owner}/{name}"))?, number))
}

/// `promptLinkPullRequestUrl`.
fn pull_request_url(link: &NewThread) -> Option<String> {
    let ((owner, name), number) = (link.repository.as_ref()?, link.pull_request?);
    Some(format!("https://github.com/{owner}/{name}/pull/{number}"))
}

/// `APP_SCHEME`: this fork's `s5code://` (what the app registers) or upstream's `t3code://`.
fn app_scheme(scheme: &str) -> bool {
    ["s5code", "t3code"].iter().any(|app| {
        let scheme = scheme.strip_prefix(app);
        scheme == Some("") || scheme.and_then(|s| s.strip_prefix('-')).is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase()))
    })
}

/// `readPairingAppLink`: `s5code://pair?pairingUrl=…`, as a pairing page hands to the desktop app.
pub fn pairing_link(url: &str) -> Option<String> {
    let parsed = s5_net::url::Url::parse(url.trim()).ok()?;
    if !app_scheme(parsed.scheme()) || parsed.host_str() != Some("pair") || !matches!(parsed.path(), "" | "/") {
        return None;
    }
    parsed.query_pairs().find(|(k, _)| k == "pairingUrl").map(|(_, v)| v.trim().to_owned()).filter(|v| !v.is_empty())
}

pub fn web_page(url: &str) -> Option<WebPage> {
    let parsed = s5_net::url::Url::parse(url.trim()).ok()?;
    match parsed.scheme() {
        "http" | "https" => Some(WebPage::Url(parsed.to_string())),
        "file" => {
            let path = parsed.to_file_path().ok()?.to_str()?.to_owned();
            is_web_page_file(&path).then_some(WebPage::File(path))
        }
        _ => None,
    }
}

/// `isWebPageFile`: `/\.x?html?$/i`.
fn is_web_page_file(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    name.rsplit_once('.').is_some_and(|(_, ext)| matches!(ext, "htm" | "html" | "xhtm" | "xhtml"))
}

/// Prompt, pairing, ChatGPT handoff and sign-in return links: what a second launch hands to
/// the running app.
pub fn is_app_link(url: &str) -> bool {
    parse(url).is_some() || thread_link(url).is_some() || pairing_link(url).is_some() || codex_callback::read_handoff(url).is_some() || s5_net::cloud::clerk::callback(url).is_some() || resume_route(url).is_some()
}

/// `resumeProviderAuth`: a sign-in page sending the person back to the app's settings.
fn resume_route(url: &str) -> Option<Route> {
    let sanitized = codex_callback::return_url(url)?;
    let path = sanitized.strip_prefix("s5code://app")?.split(['?', '#']).next().unwrap_or("");
    Some(match path.strip_prefix("/settings").map(|p| p.trim_start_matches('/')) {
        Some("") => Route::Settings("general".into()),
        Some(page) => Route::Settings(page.split('/').next().unwrap_or(page).into()),
        None => Route::Home { project_id: None },
    })
}

/// `promptLinkAppPath` + `parseNewThreadLink` for new-thread and pull-request links. `None` when
/// `url` is not one at all; thread links are not handled here.
pub fn parse(url: &str) -> Option<Result<NewThread, String>> {
    let parsed = s5_net::url::Url::parse(url.trim()).ok()?;
    let is_new = if app_scheme(parsed.scheme()) {
        parsed.host_str() == Some("new") && matches!(parsed.path(), "" | "/")
    } else {
        parsed.origin().ascii_serialization() == HOSTED_APP && parsed.path() == "/new"
    };
    if !is_new {
        return None;
    }
    let param = |k: &str| parsed.query_pairs().find(|(key, _)| key == k).map(|(_, v)| v.into_owned());
    Some((|| {
        let (prompt, hidden) = read_prompt(param("prompt"))?.unwrap_or_default();
        if let Some(pr) = param("pr") {
            let (repository, number) = pull_request(&pr).ok_or("This link names a pull request T3 Code cannot read.")?;
            return Ok(NewThread { prompt, repository: Some(repository), provider: None, model: None, pull_request: Some(number), hidden });
        }
        if prompt.is_empty() {
            return Err("This link has no prompt.".into());
        }
        let repository = match param("repo") {
            Some(repo) => Some(repository(&repo).ok_or("This link names a repository T3 Code cannot read.")?),
            None => None,
        };
        let provider = param("provider").map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty());
        let model = param("model").map(|m| m.trim().to_owned()).filter(|m| !m.is_empty() && provider.is_some());
        Ok(NewThread { prompt, repository, provider, model, pull_request: None, hidden })
    })())
}

/// `readPrompt`: a link's visible prompt and the hidden characters dropped from it.
fn read_prompt(raw: Option<String>) -> Result<Option<(String, usize)>, String> {
    let Some(raw) = raw else { return Ok(None) };
    if raw.encode_utf16().count() > MAX_PROMPT {
        return Err(format!("This prompt is longer than {} characters, so T3 Code did not open it.", "8,000"));
    }
    let (prompt, dropped) = sanitize(&raw);
    if prompt.is_empty() && dropped > 0 {
        return Err("This prompt had only hidden characters, so it may be malicious. T3 Code did not open it.".into());
    }
    Ok((!prompt.is_empty()).then_some((prompt, dropped)))
}

/// `promptLinkAppPath` for thread links: `s5code://threads/<env>/<thread>`, the hosted app's
/// `/threads/<env>/<thread>`, or its `/<env>/<thread>` when it carries a prompt.
fn thread_link(url: &str) -> Option<ThreadLink> {
    let parsed = s5_net::url::Url::parse(url.trim()).ok()?;
    let has_prompt = parsed.query_pairs().any(|(k, _)| k == "prompt");
    let path = parsed.path();
    let rest = if app_scheme(parsed.scheme()) {
        (parsed.host_str() == Some("threads")).then_some(path)?
    } else if parsed.origin().ascii_serialization() != HOSTED_APP {
        return None;
    } else if let Some(rest) = path.strip_prefix("/threads/") {
        rest
    } else if has_prompt {
        path.strip_prefix('/')?
    } else {
        return None;
    };
    let mut segments = rest.trim_start_matches('/').strip_suffix('/').unwrap_or(rest.trim_start_matches('/')).split('/');
    let (Some(env), Some(thread), None) = (segments.next(), segments.next(), segments.next()) else { return None };
    if env.is_empty() || thread.is_empty() {
        return None;
    }
    let prompt = has_prompt.then(|| read_prompt(parsed.query_pairs().find(|(k, _)| k == "prompt").map(|(_, v)| v.into_owned())));
    let prompt = match prompt {
        Some(Ok(None)) | None => None,
        Some(Ok(Some(found))) => Some(Ok(found)),
        Some(Err(e)) => Some(Err(e)),
    };
    Some(ThreadLink { thread: crate::chat::timeline::md_links::percent_decode(thread), prompt })
}

/// `resolvePromptLinkModelSelection`: the first ready (else warning) enabled instance of the
/// provider and the model it names, by slug, name or alias, else the instance's default.
/// `exact` is false when that default stands in; None when no usable instance exists.
fn link_model(s: &App, provider: &str, model: Option<&str>) -> Option<((String, String), bool)> {
    let driver = if provider == "claude" { "claudeagent" } else { provider };
    let usable: Vec<&serde_json::Value> = crate::chat::composer::providers(s)
        .iter()
        .filter(|p| data::s(p, "driver").to_lowercase() == driver && p["enabled"] == true && data::s(p, "availability") != "unavailable")
        .collect();
    let instance = usable.iter().find(|p| data::s(p, "status") == "ready").or_else(|| usable.iter().find(|p| data::s(p, "status") == "warning"))?;
    let models = instance["models"].as_array().map(Vec::as_slice).unwrap_or_default();
    let requested = model.and_then(|m| {
        let lower = m.to_lowercase();
        models
            .iter()
            .find(|o| data::s(o, "slug") == m)
            .or_else(|| models.iter().find(|o| data::s(o, "name").to_lowercase() == lower))
            .or_else(|| models.iter().find(|o| o["aliases"].as_array().is_some_and(|a| a.iter().any(|v| v.as_str().is_some_and(|v| v.to_lowercase() == lower)))))
    });
    let picked = requested.or_else(|| models.iter().find(|o| o["isDefault"] == true)).or_else(|| models.first())?;
    Some(((data::s(instance, "instanceId").to_owned(), data::s(picked, "slug").to_owned()), model.is_none() || requested.is_some()))
}

/// Opens a prompt link from anywhere (a second launch, the OS's URL handler), on the UI thread:
/// the draft lands now, or once the shell has loaded. False when `url` is no prompt link.
pub fn open(s: &mut App, url: &str) -> bool {
    if let Some(handoff) = codex_callback::read_handoff(url) {
        codex_callback::handoff(s, handoff);
        return true;
    }
    // Before `resume_route`, which reads any `s5code://app/` link as the app's home.
    if let Some(callback) = s5_net::cloud::clerk::callback(url) {
        crate::cloud::returned(s, callback);
        return true;
    }
    if let Some(route) = resume_route(url) {
        s.navigate(route);
        return true;
    }
    if let Some(pairing_url) = pairing_link(url) {
        // A repeated click would only ask again and then fail on the spent one-time token.
        if s.ui.links.paired.contains(&pairing_url) {
            return true;
        }
        match s5_net::resolve_pairing(&pairing_url, "") {
            Ok((origin, _)) => s.ui.links.pair = Some((pairing_url, origin)),
            Err(e) => crate::shell::palette::toast(s, "Could not add backend", Some(&e)),
        }
        return true;
    }
    if let Some(link) = thread_link(url) {
        s.ui.links.pending_thread = Some(link);
        poll(s);
        return true;
    }
    let Some(link) = parse(url) else {
        let Some(page) = web_page(url) else { return false };
        s.ui.links.web = Web::Pending(page);
        poll_web(s);
        return true;
    };
    s.ui.links.pending = Some(link);
    poll(s);
    true
}

/// Links passed on the command line at launch.
pub fn start(s: &mut App) {
    if let Some(url) = std::env::args().skip(1).find(|a| is_app_link(a)) {
        open(s, &url);
    }
}

/// macOS hands links to the running app.
pub fn on_open_url(s: &mut App, url: &str, _: &mut rune::Tx) {
    open(s, url);
}

/// `findPromptLinkProjects`: the repository's projects, most recently updated first.
fn projects_for(s: &App, (owner, name): &(String, String)) -> Vec<String> {
    let path = format!("{owner}/{name}").to_lowercase();
    let keys: Vec<String> = REPOSITORY_HOSTS.iter().map(|h| format!("{h}/{path}")).collect();
    let mut projects: Vec<&serde_json::Value> = s
        .state()
        .projects
        .iter()
        .filter(|p| p["archivedAt"].is_null())
        .filter(|p| {
            let identity = &p["repositoryIdentity"];
            let key = identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str()).unwrap_or_default().to_lowercase();
            keys.contains(&key)
        })
        .collect();
    projects.sort_by_key(|p| std::cmp::Reverse(data::ts(p, "updatedAt").unwrap_or(0)));
    projects.into_iter().map(|p| data::s(p, "id").to_owned()).collect()
}

/// `findPromptLinkPullRequestThread`: the latest-updated thread linked to the pull request,
/// leaving out side chats and dismissed stack links.
fn pull_request_thread(s: &App, (owner, name): &(String, String), number: u64) -> Option<String> {
    let repository = format!("{owner}/{name}");
    s.state()
        .threads
        .iter()
        .filter(|t| t["lineage"]["sideChat"] != true)
        .filter(|t| {
            t["pullRequests"].as_array().is_some_and(|links| {
                links.iter().any(|l| {
                    data::s(l, "host").eq_ignore_ascii_case("github.com")
                        && data::s(l, "repository").eq_ignore_ascii_case(&repository)
                        && l["number"].as_u64() == Some(number)
                        && data::s(l, "source") != "stack-dismissed"
                })
            })
        })
        .max_by_key(|t| data::ts(t, "updatedAt").unwrap_or(i64::MIN))
        .map(|t| data::s(t, "id").to_owned())
}

/// `buildNewThreadLink` through `promptLinkFor`: the hosted app's `/new` link for a draft's prompt,
/// or `None` for an empty prompt or one past `PROMPT_LINK_MAX_PROMPT_LENGTH`.
pub fn new_thread_link(prompt: &str, repository: Option<&(String, String)>, provider: Option<&str>, model: Option<&str>) -> Option<String> {
    let prompt = prompt.trim();
    if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT {
        return None;
    }
    let mut url = s5_net::url::Url::parse(HOSTED_APP).ok()?.join("/new").ok()?;
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("prompt", prompt);
        if let Some((owner, name)) = repository {
            q.append_pair("repo", &format!("{owner}/{name}"));
        }
        if let Some(provider) = provider.filter(|p| !p.is_empty()) {
            q.append_pair("provider", provider);
            if let Some(model) = model.filter(|m| !m.is_empty()) {
                q.append_pair("model", model);
            }
        }
    }
    Some(url.to_string())
}

/// `promptLinkRepositoryOf`: a GitHub project's `owner/name`, by its grouping key.
pub fn repository_of(project: &serde_json::Value) -> Option<(String, String)> {
    let identity = &project["repositoryIdentity"];
    let key = identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str())?;
    repository(key.strip_prefix("github.com/")?)
}

/// Lands a waiting link once the shell has loaded projects.
/// Web's `pairFromLink`: any page can open a pairing link, so the person confirms the backend
/// before its one-time token is spent.
pub fn overlays(s: &App, t: &crate::theme::T, layers: &mut Vec<rune::El>) {
    landing_dialog(s, t, layers);
    let Some((_, origin)) = &s.ui.links.pair else { return };
    let cancel = |s: &mut App| s.ui.links.pair = None;
    crate::kit::Dialog::new("pairing-link", &format!("Add the backend at {origin}?"))
        .open(true)
        .alert()
        .on_close(cancel)
        .child(
            rune::row()
                .gap(8.0)
                .child(crate::kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(crate::kit::Button::new("Confirm").on_press(confirm_pairing).build(t)),
        )
        .build(t, layers);
}

/// NewThreadLinkView's missing-repository page, and its "Opening the link..." while the clone
/// starts, as a dialog over whatever the window shows.
fn landing_dialog(s: &App, t: &crate::theme::T, layers: &mut Vec<rune::El>) {
    use crate::kit::Button;
    let dialog = match &s.ui.links.landing {
        Some(Landing::Missing(NewThread { repository: Some((owner, name)), .. })) => {
            let full = format!("{owner}/{name}");
            crate::kit::Dialog::new("prompt-link", format!("You don't have {full}"))
                .description("Clone it to work on it here, or start without a project.")
                .child(
                    rune::row()
                        .w_full()
                        .gap(8.0)
                        .justify_end()
                        .child(Button::new(format!("Clone {full}")).size("sm").on_press(clone_repository).build(t))
                        .child(Button::new("Use without project").size("sm").variant("outline").on_press(use_without_project).build(t)),
                )
        }
        Some(Landing::Checkout { link: NewThread { repository: Some((owner, name)), pull_request: Some(number), .. }, started, .. }) => {
            crate::kit::Dialog::new("prompt-link", format!("Check out {owner}/{name}#{number}?"))
                .description("This puts the pull request's code in a new worktree and runs your project's setup script on it.")
                .child(
                    rune::row().w_full().justify_end().child(
                        Button::new(if *started { "Checking out..." } else { "Check out" }).size("sm").disabled(*started).on_press(check_out).build(t),
                    ),
                )
        }
        Some(Landing::Cloning { .. } | Landing::Awaiting { .. } | Landing::Lookup { .. }) => crate::kit::Dialog::new("prompt-link", "Opening the link...")
            .child(rune::row().child(crate::kit::Spinner::new().size("sm").tone("muted").build(&crate::kit::theme(t)))),
        _ => return,
    };
    dialog.open(true).width(448.0).on_close(|s: &mut App| s.ui.links.landing = None).build(t, layers);
}

/// `lastActiveEnvironmentId`: where the latest-updated thread lives, else this machine while
/// it is on.
fn last_active_environment(s: &App) -> Option<String> {
    match s.state().threads.iter().max_by_key(|t| data::ts(t, "updatedAt").unwrap_or(i64::MIN)) {
        Some(thread) => Some(crate::env::of(s, thread)),
        None => (!s.ui.desktop.local_off).then(|| crate::env::LOCAL.to_owned()),
    }
}

/// `startProjectClone`'s input for a link's GitHub repository, cloned under `root`.
fn clone_payload((owner, name): &(String, String), root: &str, project: &str) -> serde_json::Value {
    use crate::shell::palette::add_project::{clone_destination, iso_now};
    serde_json::json!({
        "projectId": project,
        "title": name,
        "createdAt": iso_now(),
        "remoteUrl": format!("https://github.com/{owner}/{name}.git"),
        "destinationPath": clone_destination(root, name),
    })
}

/// `cloneRepository`: clones the link's repository into the last active machine's new-projects
/// folder, then opens the draft there.
fn clone_repository(s: &mut App) {
    let Some(Landing::Missing(link)) = s.ui.links.landing.take() else { return };
    let Some(repository) = &link.repository else { return };
    let env = last_active_environment(s);
    let root = env
        .as_deref()
        .and_then(|key| crate::env::source(s, key))
        .and_then(|src| src.state().config.as_ref())
        .and_then(|config| config["newProjectsRoot"].as_str())
        .filter(|root| !root.is_empty())
        .map(str::to_owned);
    let (Some(env), Some(root)) = (env, root) else {
        return crate::shell::palette::toast(s, "Could not open the link", Some(CANNOT_CLONE));
    };
    let project = s5_net::uuid();
    let payload = clone_payload(repository, &root, &project);
    match crate::env::net_of(s, &env).map(|net| net.request("projectClone.start", payload)) {
        Some(request) => s.ui.links.landing = Some(Landing::Cloning { request, env, project, link }),
        None => crate::shell::palette::toast(s, "Could not open the link", Some(CANNOT_CLONE)),
    }
}

fn use_without_project(s: &mut App) {
    let Some(Landing::Missing(link)) = s.ui.links.landing.take() else { return };
    start_scratch(s, link);
}

/// `openScratchDraft`: a link without a repository, or without its project, drafts in the
/// Scratch project.
fn start_scratch(s: &mut App, link: NewThread) {
    use crate::shell::palette::actions;
    if actions::scratch_available(s) {
        actions::new_without_project(s);
    }
    if actions::scratch_pending(s) {
        s.ui.links.landing = Some(Landing::Scratch(link));
    } else {
        crate::shell::palette::toast(s, "Could not open the link", Some(NO_SCRATCH));
    }
}

/// Folds in the clone reply, or places the prompt once the scratch project has opened.
fn poll_landing(s: &mut App) -> bool {
    use crate::shell::palette::actions;
    match s.ui.links.landing.take() {
        Some(Landing::Cloning { request, env, project, link }) => {
            let mut reply = crate::env::net_of(s, &env).and_then(|net| net.take_reply(request));
            if reply.is_none() {
                reply = s.src.net().and_then(|net| net.take_reply(request));
            }
            match reply {
                None => {
                    s.ui.links.landing = Some(Landing::Cloning { request, env, project, link });
                    false
                }
                Some(Err(e)) => {
                    let message = crate::shell::palette::error_message(&e, "An error occurred.");
                    crate::shell::palette::toast(s, "Clone failed", Some(&message));
                    s.ui.links.landing = Some(Landing::Missing(link));
                    true
                }
                Some(Ok(_)) => {
                    crate::env::adopt(s, &project, &env);
                    s.ui.links.landing = Some(Landing::Awaiting { project, link, deadline: std::time::Instant::now() + CLONE_WAIT });
                    // The deadline passes without anything else waking the window.
                    if let Some(waker) = s.ui.panel.waker.clone() {
                        std::thread::spawn(move || {
                            std::thread::sleep(CLONE_WAIT);
                            waker();
                        });
                    }
                    poll_landing(s);
                    true
                }
            }
        }
        Some(Landing::Awaiting { project, link, .. }) if crate::chat::composer::project_by_id(s, &project).is_some() => {
            match (&link.repository, link.pull_request) {
                (Some((owner, name)), Some(_)) => crate::shell::palette::toast(s, &format!("Cloning {owner}/{name}"), Some("Open this link again when the clone finishes.")),
                _ => {
                    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project) });
                    land_draft(s, &link, None);
                }
            }
            true
        }
        Some(Landing::Awaiting { deadline, .. }) if std::time::Instant::now() >= deadline => {
            crate::shell::palette::toast(s, "Could not open the link", Some("The clone started, but its project did not appear."));
            true
        }
        Some(Landing::Scratch(link)) if actions::scratch_pending(s) => {
            s.ui.links.landing = Some(Landing::Scratch(link));
            false
        }
        // The scratch flow reports its own failure.
        Some(Landing::Scratch(link)) => {
            let landed = matches!(&s.route, Route::Home { project_id: Some(id) }
                if crate::chat::composer::project_by_id(s, id).is_some_and(|p| actions::is_scratch_project(s, p)));
            if landed {
                land_draft(s, &link, pull_request_url(&link));
            }
            true
        }
        Some(Landing::Lookup { mut requests, link, project }) => {
            for (env, request, reply) in &mut requests {
                if reply.is_none() {
                    *reply = crate::env::net_of(s, env).and_then(|net| net.take_reply(*request));
                }
                if reply.is_none() {
                    *reply = s.src.net().and_then(|net| net.take_reply(*request));
                }
            }
            match pick_linked_thread(&requests) {
                None => {
                    s.ui.links.landing = Some(Landing::Lookup { requests, link, project });
                    false
                }
                Some(Some(thread)) => {
                    open_thread(s, &thread, &link.prompt, link.hidden);
                    true
                }
                Some(None) => {
                    s.ui.links.landing = Some(Landing::Checkout { link, project, started: false });
                    true
                }
            }
        }
        // The handoff reports how the checkout went.
        Some(Landing::Checkout { started: true, .. }) if s.ui.chat.details.handoff.is_none() => true,
        other => {
            s.ui.links.landing = other;
            false
        }
    }
}

fn confirm_pairing(s: &mut App) {
    let Some((pairing_url, _)) = s.ui.links.pair.take() else { return };
    if crate::env::pairing(s) {
        return crate::shell::palette::toast(s, "Could not add backend", Some("Another environment is pairing. Try the link again in a moment."));
    }
    s.ui.env.host = rune::TextEdit::new(pairing_url.clone());
    s.ui.env.code = rune::TextEdit::default();
    crate::env::add(s);
    s.ui.links.paired.push(pairing_url);
    s.ui.links.pairing = true;
}

/// Says how a confirmed pairing link went once `env` lands it.
fn poll_pairing(s: &mut App) -> bool {
    if !s.ui.links.pairing || crate::env::pairing(s) {
        return false;
    }
    s.ui.links.pairing = false;
    match s.ui.env.error.take() {
        Some(error) => {
            s.ui.links.paired.pop();
            crate::shell::palette::toast(s, "Could not add backend", Some(&error));
        }
        None => crate::shell::palette::toast(s, "Backend added", Some("The environment is saved and will reconnect on app startup.")),
    }
    true
}

fn web_failed(s: &mut App, page: &WebPage, description: Option<&str>) {
    let shown = match page {
        WebPage::Url(url) | WebPage::File(url) => url,
    };
    crate::shell::palette::toast(s, "Could not open the link", Some(description.unwrap_or(shown)));
}

/// `openInBrowser`: the thread showing a browser takes the page, else a new scratch thread
/// does. Only this machine's server serves files, so a file needs it active.
fn poll_web(s: &mut App) -> bool {
    use crate::shell::palette::actions;
    match std::mem::take(&mut s.ui.links.web) {
        Web::Idle => false,
        Web::Pending(page) if !s.state().shell_ready => {
            s.ui.links.web = Web::Pending(page);
            false
        }
        Web::Pending(page) => {
            // "Open pull requests in T3 Code" covers links the OS hands over too.
            if let WebPage::Url(url) = &page
                && crate::panel::pull_requests::open_in_app(s, url)
            {
                return true;
            }
            if matches!(page, WebPage::File(_)) && (s.ui.env.active != crate::env::LOCAL || s.ui.desktop.local_off) {
                web_failed(s, &page, Some("HTML files open on this machine's environment."));
            } else if crate::panel::showing_browser(s) {
                show_web(s, page);
            } else if actions::scratch_available(s) {
                actions::new_without_project(s);
                s.ui.links.web = Web::Scratch(page);
            } else {
                web_failed(s, &page, None);
            }
            true
        }
        Web::Scratch(page) if actions::scratch_pending(s) => {
            s.ui.links.web = Web::Scratch(page);
            false
        }
        // The scratch flow reports its own failure.
        Web::Scratch(page) => {
            if matches!(s.route, Route::Home { project_id: Some(_) }) {
                show_web(s, page);
            }
            true
        }
        Web::Asset(request, path) => {
            let Some(net) = s.src.net() else {
                s.ui.links.web = Web::Asset(request, path);
                return false;
            };
            let Some(reply) = net.take_reply(request) else {
                s.ui.links.web = Web::Asset(request, path);
                return false;
            };
            let base = net.base_url().trim_end_matches('/').to_owned();
            match reply {
                Ok(v) => crate::panel::open_preview(s, &format!("{base}{}", data::s(&v, "relativeUrl"))),
                Err(e) => {
                    let message = crate::shell::palette::error_message(&e, &path);
                    web_failed(s, &WebPage::File(path), Some(&message));
                }
            }
            true
        }
    }
}

/// An absolute `media-file` is served without looking up its thread (`assetsCreateUrl`), so a
/// draft passes a fresh id.
fn show_web(s: &mut App, page: WebPage) {
    match page {
        WebPage::Url(url) => crate::panel::open_preview(s, &url),
        WebPage::File(path) => {
            let thread = match &s.route {
                Route::Thread(id) => id.clone(),
                _ => s5_net::uuid(),
            };
            let resource = serde_json::json!({"resource":{"_tag":"media-file","threadId":thread,"path":path}});
            match s.src.net().map(|net| net.request("assets.createUrl", resource)) {
                Some(request) => s.ui.links.web = Web::Asset(request, path),
                None => web_failed(s, &WebPage::File(path), None),
            }
        }
    }
}

/// `pickPromptLinkPullRequestThread`: the first unarchived thread, in order, once every lookup
/// before it has answered. `Some(None)` when none has one; a failed lookup has none.
fn pick_linked_thread(lookups: &[(String, u64, Option<s5_net::Reply>)]) -> Option<Option<String>> {
    for (_, _, reply) in lookups {
        let threads: &[serde_json::Value] = match reply.as_ref()? {
            Ok(result) => result["threads"].as_array().map(Vec::as_slice).unwrap_or_default(),
            Err(_) => &[],
        };
        if let Some(thread) = threads.iter().find(|t| t["archivedAt"].is_null()) {
            return Some(Some(data::s(thread, "id").to_owned()));
        }
    }
    Some(None)
}

/// Opens a thread the link points at, with the link's prompt in its composer.
fn open_thread(s: &mut App, thread: &str, prompt: &str, hidden: usize) {
    crate::shell::sidebar::open_thread(s, thread);
    if !prompt.is_empty() {
        place_prompt(s, prompt, hidden, None);
    }
}

/// `openDraft`: the link's prompt (and `extra`) and model on the draft the window now shows.
fn land_draft(s: &mut App, link: &NewThread, extra: Option<String>) {
    let text = [Some(link.prompt.as_str()).filter(|p| !p.is_empty()), extra.as_deref()].into_iter().flatten().collect::<Vec<_>>().join("\n\n");
    let note = link.provider.as_deref().and_then(|provider| apply_link_model(s, provider, link.model.as_deref()));
    if !text.is_empty() {
        place_prompt(s, &text, link.hidden, note);
    } else if let Some(note) = note {
        crate::shell::palette::toast(s, "Prompt added from a link", Some(&format!("Read it before you send. {note}")));
    }
}

/// `placeLinkedPrompt`: the prompt fills the draft the window shows, asking first before it
/// replaces a different one. Hidden characters it dropped warn until dismissed.
fn place_prompt(s: &mut App, prompt: &str, hidden: usize, note: Option<String>) {
    let key = crate::chat::composer::draft_key(s, s.frame_route());
    let existing = s.ui.chat.composer.drafts.get(&key).map(|d| d.edit.text.clone()).unwrap_or_default();
    let replaces = !existing.trim().is_empty() && existing != prompt;
    let text = prompt.to_owned();
    if hidden > 0 {
        let plural = if hidden == 1 { "" } else { "s" };
        let description = format!("T3 Code removed {hidden} hidden character{plural}. Read the prompt before you send it.");
        crate::shell::palette::toast_warning_sticky(s, "This prompt had hidden characters, it may be malicious", Some(&description));
    }
    let place = std::rc::Rc::new(move |s: &mut App| {
        crate::shell::toast::close_titled(s, "Linked prompt ready");
        let draft = s.ui.chat.composer.drafts.entry(key.clone()).or_default();
        draft.edit.replace_text(&text);
        draft.edit.set_caret(text.len());
        let description = ["Read it before you send.", note.as_deref().unwrap_or_default()].join(" ");
        crate::shell::palette::toast(s, "Prompt added from a link", Some(description.trim_end()));
    });
    if !replaces {
        return place(s);
    }
    let action: crate::shell::palette::ToastAction = ("Replace draft".into(), true, std::rc::Rc::new(move |s: &mut App, _: &mut rune::Tx| place(s)));
    crate::shell::palette::toast_sticky_with(s, "Linked prompt ready", Some("This thread already has a draft. Replace it with the linked prompt?"), vec![action]);
}

/// `PullRequestLinkCheckout`'s button: the handoff runs on the project's draft.
fn check_out(s: &mut App) {
    let Some(Landing::Checkout { link, project, started: false }) = &s.ui.links.landing else { return };
    let (Some(url), prompt, project) = (pull_request_url(link), link.prompt.clone(), project.clone()) else { return };
    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project) });
    crate::chat::details::pr::start_link_handoff(s, &url, &prompt);
    let pending = s.ui.chat.details.handoff.is_some();
    if let Some(Landing::Checkout { started, .. }) = &mut s.ui.links.landing {
        *started = pending;
    }
}

/// NewThreadLinkView's decision once projects have loaded.
fn open_link(s: &mut App, link: NewThread) {
    let Some(repository) = link.repository.clone() else { return start_scratch(s, link) };
    if let Some(thread) = link.pull_request.and_then(|number| pull_request_thread(s, &repository, number)) {
        return open_thread(s, &thread, &link.prompt, link.hidden);
    }
    let projects = projects_for(s, &repository);
    let Some(project) = projects.first().cloned() else {
        s.ui.links.landing = Some(Landing::Missing(link));
        return;
    };
    let Some(number) = link.pull_request else {
        s.navigate(Route::Home { project_id: Some(project) });
        return land_draft(s, &link, None);
    };
    let requests = projects
        .iter()
        .map(|id| {
            let env = crate::env::owner(s, id);
            let payload = serde_json::json!({"projectId": id, "repository": format!("{}/{}", repository.0, repository.1), "number": number});
            match crate::env::net_of(s, &env).map(|net| net.request("pullRequests.linkedThreads", payload)) {
                Some(request) => (env, request, None),
                None => (env, 0, Some(Err(serde_json::Value::Null))),
            }
        })
        .collect();
    s.ui.links.landing = Some(Landing::Lookup { requests, link, project });
    poll_landing(s);
}

/// A link's `provider` and `model` pick the open draft's model, and the note when they can't.
fn apply_link_model(s: &mut App, provider: &str, model: Option<&str>) -> Option<String> {
    let resolved = link_model(s, provider, model);
    if let Some((selection, _)) = &resolved {
        let key = crate::chat::composer::draft_key(s, s.frame_route());
        let draft = s.ui.chat.composer.drafts.entry(key).or_default();
        draft.model = Some(selection.clone());
        draft.effort = None;
    }
    match resolved {
        None => Some(format!("The link asked for {provider}, which is not set up here, so your default model is used.")),
        Some((_, false)) => Some(format!("The link asked for {}, which {provider} does not offer here, so its default model is used.", model.unwrap_or_default())),
        Some(_) => None,
    }
}

pub fn poll(s: &mut App) -> bool {
    let paired = poll_pairing(s) | poll_web(s) | poll_landing(s);
    if !s.state().shell_ready {
        return paired;
    }
    // `ThreadLinkPrompt`: the thread opens, and its prompt lands in the composer.
    if let Some(link) = s.ui.links.pending_thread.take() {
        crate::shell::sidebar::open_thread(s, &link.thread);
        match link.prompt {
            Some(Err(message)) => crate::shell::palette::toast_error(s, "Could not open the linked prompt", Some(&message)),
            Some(Ok((prompt, hidden))) => place_prompt(s, &prompt, hidden, None),
            None => {}
        }
        return true;
    }
    if s.ui.links.pending.is_none() {
        return paired;
    }
    match s.ui.links.pending.take().expect("checked above") {
        Err(message) => crate::shell::palette::toast(s, "Could not open the link", Some(&message)),
        Ok(link) => open_link(s, link),
    }
    true
}

/// A prompt link for a repository no project has, over the "New thread" draft.
fn scene_missing_repository(s: &mut App) {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == "New thread").map(|t| data::s(t, "id").to_owned()) {
        s.navigate(Route::Thread(id));
    }
    let repository = Some(("acme".to_owned(), "widgets".to_owned()));
    s.ui.links.landing = Some(Landing::Missing(NewThread { prompt: "Fix the flaky tests".into(), repository, provider: None, model: None, pull_request: None, hidden: 0 }));
}

/// A pull request link with no thread on it yet: the checkout offer.
fn scene_pull_request_checkout(s: &mut App) {
    scene_missing_repository(s);
    let link = NewThread { prompt: String::new(), repository: Some(("acme".into(), "widgets".into())), provider: None, model: None, pull_request: Some(128), hidden: 0 };
    s.ui.links.landing = Some(Landing::Checkout { link, project: String::new(), started: false });
}

pub const SCENES: &[(&str, fn(&mut App))] =
    &[("link-missing-repository", scene_missing_repository), ("link-pull-request-checkout", scene_pull_request_checkout)];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_links_provider_and_model_pick_the_drafts_model() {
        let mut s = App::new();
        // A link without a repository drafts in Scratch, which this fixture lacks, so name one it has.
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.projects[0]["repositoryIdentity"] = serde_json::json!({"canonicalKey": "github.com/acme/app"});
        }
        let model = |s: &App| s.ui.chat.composer.drafts.get(&crate::chat::composer::draft_key(s, s.frame_route())).and_then(|d| d.model.clone());
        let toast = |s: &App| crate::shell::palette::toast_title(s).map(str::to_owned);
        let description = |s: &App| crate::shell::palette::toast_description(s).map(str::to_owned);
        assert!(open(&mut s, "s5code://new?prompt=hi&repo=acme/app&provider=OpenCode&model=opencode%2Fbig-pickle"));
        assert_eq!(model(&s), Some(("opencode".into(), "opencode/big-pickle".into())), "FAILED: the named model is picked");
        assert_eq!((toast(&s).as_deref(), description(&s).as_deref()), (Some("Prompt added from a link"), Some("Read it before you send.")), "FAILED: an exact match needs no note");
        s.ui.chat.composer.drafts.clear();
        assert!(open(&mut s, "s5code://new?prompt=again&repo=acme/app&provider=opencode&model=nope"));
        assert_eq!(model(&s), Some(("opencode".into(), "opencode/beam-free".into())), "FAILED: an unknown model falls back to the instance's first");
        assert!(description(&s).is_some_and(|d| d.contains("does not offer here")), "FAILED: the model note joins the toast");
        assert!(open(&mut s, "s5code://new?prompt=more&repo=acme/app&provider=claude&model=claude-opus-5-5"));
        assert_eq!(model(&s), Some(("opencode".into(), "opencode/beam-free".into())), "FAILED: a provider not ready here leaves the model alone");
    }

    #[test]
    fn sign_in_return_links_resume_in_settings() {
        assert_eq!(resume_route("s5code://app/settings/providers?instanceId=work"), Some(Route::Settings("providers".into())));
        assert_eq!(resume_route("s5code://app/settings"), Some(Route::Settings("general".into())));
        assert_eq!(resume_route("s5code://app/welcome#agents:env-1"), Some(Route::Home { project_id: None }));
        for other in ["t3code://app/settings", "s5code://app/threads/1", "https://app.t3.codes/settings", "s5code://new?prompt=x"] {
            assert_eq!(resume_route(other), None, "{other}");
        }
        assert!(is_app_link("s5code://app/settings/providers"));
    }

    #[test]
    fn web_pages_follow_electrons_link_and_file_rules() {
        assert_eq!(web_page("https://example.com/a?b=1"), Some(WebPage::Url("https://example.com/a?b=1".into())));
        assert_eq!(web_page("http://localhost:3000"), Some(WebPage::Url("http://localhost:3000/".into())));
        // A Windows file URL names a drive.
        let (root, path) = if cfg!(windows) { ("file:///C:", "C:\\home\\a b\\Index.HTML") } else { ("file://", "/home/a b/Index.HTML") };
        assert_eq!(web_page(&format!("{root}/home/a%20b/Index.HTML")), Some(WebPage::File(path.into())));
        for page in ["/x/page.htm", "/x/page.xhtml", "/x/page.xhtm"] {
            assert!(matches!(web_page(&format!("{root}{page}")), Some(WebPage::File(_))), "{page}");
        }
        for other in ["file:///x/notes.md", "file:///x.html/notes", "file:///x/html", "mailto:a@b.c", "s5code://new?prompt=x", "ftp://a/b.html"] {
            assert_eq!(web_page(other), None, "{other}");
        }
    }

    #[test]
    fn prompt_links_parse_like_the_web() {
        let ok = |url: &str| parse(url).expect("a prompt link").expect("valid");
        let link = ok("t3code://new?prompt=Fix%20the%20tests&repo=pingdotgg/t3code");
        assert_eq!(link, NewThread { prompt: "Fix the tests".into(), repository: Some(("pingdotgg".into(), "t3code".into())), provider: None, model: None, pull_request: None, hidden: 0 });
        assert_eq!(ok("https://app.s5code.touchtech.club/new?prompt=hi&repo=https://github.com/a/b.git").repository, Some(("a".into(), "b".into())));
        assert_eq!(ok("t3code-dev://new/?prompt=%20hi%E2%80%8B%20").prompt, "hi");
        assert_eq!(ok("s5code://new?prompt=hi").prompt, "hi");
        assert_eq!(ok("s5code-dev://new?prompt=hi").prompt, "hi");
        assert!(parse("s5codex://new?prompt=hi").is_none());
        assert_eq!(ok("t3code://new?prompt=a%0D%0Ab").prompt, "a\nb");
        assert_eq!(ok("t3code://new?prompt=%F0%9F%91%A9%E2%80%8D%F0%9F%92%BB").prompt, "\u{1F469}\u{200D}\u{1F4BB}");
        let err = |url: &str| parse(url).expect("a prompt link").unwrap_err();
        assert_eq!(err("t3code://new?prompt=%E2%80%8B"), "This prompt had only hidden characters, so it may be malicious. T3 Code did not open it.");
        assert_eq!(err("t3code://new"), "This link has no prompt.");
        assert_eq!(err("t3code://new?prompt=x&repo=not%20a%20repo"), "This link names a repository T3 Code cannot read.");
        assert!(err(&format!("t3code://new?prompt={}", "x".repeat(8_001))).contains("longer than 8,000"));
        assert!(parse("https://example.com/new?prompt=x").is_none());
        assert!(parse("https://app.t3.codes/new?prompt=x").is_none(), "upstream's hosted app is not this fork's");
        assert!(parse("t3code://pair?pairingUrl=x").is_none());
        // `readPairingAppLink`.
        assert_eq!(pairing_link("s5code://pair?pairingUrl=https%3A%2F%2Fbox.example%2Fpair%23token%3Dabc").as_deref(), Some("https://box.example/pair#token=abc"));
        assert_eq!(pairing_link("t3code-dev://pair/?pairingUrl= https://box/ ").as_deref(), Some("https://box/"));
        assert_eq!(pairing_link("s5code://pair?pairingUrl="), None);
        assert_eq!(pairing_link("s5code://pair/extra?pairingUrl=https://box"), None);
        assert_eq!(pairing_link("https://pair?pairingUrl=https://box"), None);
        assert!(is_app_link("s5code://pair?pairingUrl=https://box") && is_app_link("s5code://new?prompt=hi") && !is_app_link("s5code://other"));
        assert!(parse("--check-server").is_none());
        // `parsePromptLinkPullRequest`: the prompt is optional and provider/model don't apply.
        let acme = Some(("acme".to_string(), "app".to_string()));
        let pr = ok("s5code://new?pr=https://github.com/acme/app/pull/12/files&provider=codex");
        assert_eq!(pr, NewThread { prompt: String::new(), repository: acme.clone(), provider: None, model: None, pull_request: Some(12), hidden: 0 });
        assert_eq!(pull_request_url(&pr).as_deref(), Some("https://github.com/acme/app/pull/12"));
        let short = ok("s5code://new?pr=acme/app%237&prompt=Review%20it");
        assert_eq!((short.repository, short.pull_request, short.prompt.as_str()), (acme.clone(), Some(7), "Review it"));
        assert_eq!(ok("s5code://new?pr=HTTPS://www.GitHub.com/acme/app.git/PULL/3?w=1").pull_request, Some(3));
        assert_eq!(ok("s5code://new?pr=acme/app/9").repository, acme);
        for bad in ["acme/app", "acme/app/0", "acme/app/1x", "acme/app/+1", "https://gitlab.com/a/b/pull/1", "https://github.com/a/b/issues/1", "a b/c/1", "acme/app/9007199254740992"] {
            assert_eq!(err(&format!("s5code://new?pr={}", bad.replace('#', "%23"))), "This link names a pull request T3 Code cannot read.", "{bad}");
        }
        assert_eq!(err("s5code://new?pr=acme/app/1&prompt=%E2%80%8B"), "This prompt had only hidden characters, so it may be malicious. T3 Code did not open it.");
    }

    fn ready(projects: Vec<serde_json::Value>, threads: Vec<serde_json::Value>) -> App {
        let mut s = App::new();
        let mut state = crate::data::empty_state();
        state.shell_ready = true;
        state.projects = projects;
        state.threads = threads;
        s.src = crate::data::Source::Fixture(state);
        s
    }

    #[test]
    fn os_handed_pull_request_links_follow_the_open_in_t3_code_setting() {
        use serde_json::json;
        let projects = vec![json!({"id": "p1", "repositoryIdentity": {"canonicalKey": "github.com/acme/app"}})];
        let mut s = ready(projects.clone(), vec![]);
        assert!(open(&mut s, "https://github.com/acme/app/pull/12"));
        assert_eq!(s.route, Route::PullRequests, "FAILED: a pull request the OS hands over skips the pull requests view");
        assert!(matches!(s.ui.links.web, Web::Idle));
        let mut off = ready(projects, vec![]);
        off.client_settings.values.insert("pullRequestLinksOpenInApp".into(), json!(false));
        open(&mut off, "https://github.com/acme/app/pull/12");
        assert_ne!(off.route, Route::PullRequests, "off leaves it to the browser");

        let sign_in = "s5code://app/?__clerk_oauth_attempt=a-1&rotating_token_nonce=n-1";
        assert!(is_app_link(sign_in));
        let mut s = ready(vec![], vec![]);
        s.navigate(Route::Settings("connections".into()));
        assert!(open(&mut s, sign_in));
        assert_eq!(s.route, Route::Settings("connections".into()), "a T3 Connect sign-in return is not the app's home link");
    }

    #[test]
    fn pull_request_links_open_the_linked_thread_else_offer_the_checkout() {
        use serde_json::json;
        let pr = |number: u64, extra: serde_json::Value| {
            let mut link = json!({"host": "github.com", "repository": "Acme/App", "number": number});
            link.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            link
        };
        let mut s = ready(
            vec![json!({"id": "p1", "repositoryIdentity": {"canonicalKey": "github.com/acme/app"}, "updatedAt": "2026-01-01T00:00:00.000Z"})],
            vec![
                json!({"id": "t1", "updatedAt": "2026-01-02T00:00:00.000Z", "pullRequests": [pr(12, json!({}))]}),
                json!({"id": "side", "updatedAt": "2026-01-03T00:00:00.000Z", "lineage": {"sideChat": true}, "pullRequests": [pr(12, json!({}))]}),
                json!({"id": "dismissed", "updatedAt": "2026-01-04T00:00:00.000Z", "pullRequests": [pr(12, json!({"source": "stack-dismissed"}))]}),
                json!({"id": "gitlab", "updatedAt": "2026-01-05T00:00:00.000Z", "pullRequests": [pr(12, json!({"host": "gitlab.com"}))]}),
            ],
        );
        s.ui.links.pending = parse("s5code://new?pr=https://github.com/acme/app/pull/12&prompt=Review");
        assert!(poll(&mut s));
        assert_eq!(s.route, Route::Thread("t1".into()), "FAILED: the latest visible thread on the pull request opens");
        let text = |s: &App| s.ui.chat.composer.drafts.get(&crate::chat::composer::draft_key(s, s.frame_route())).map(|d| d.edit.text.clone());
        assert_eq!(text(&s).as_deref(), Some("Review"));
        // No local thread: each project's machine is asked; one that can't answer has none.
        s.ui.links.pending = parse("s5code://new?pr=acme/app/13");
        assert!(poll(&mut s));
        assert!(matches!(&s.ui.links.landing, Some(Landing::Checkout { project, started: false, .. }) if project == "p1"), "{:?}", s.ui.links.landing);
        check_out(&mut s);
        assert_eq!(s.route, Route::Home { project_id: Some("p1".into()) });
        assert!(matches!(&s.ui.links.landing, Some(Landing::Checkout { started: false, .. })), "FAILED: a fixture has no server to prepare the checkout, so the button stays");
        s.ui.links.landing = None;
        s.ui.links.pending = parse("s5code://new?pr=other/repo/1");
        assert!(poll(&mut s));
        assert!(matches!(&s.ui.links.landing, Some(Landing::Missing(NewThread { pull_request: Some(1), .. }))));
        // `pickPromptLinkPullRequestThread`.
        let lookup = |reply: Option<s5_net::Reply>| (String::new(), 0, reply);
        let threads = |id: &str, archived: serde_json::Value| Some(Ok(json!({"threads": [{"id": id, "archivedAt": archived}]})));
        assert_eq!(pick_linked_thread(&[lookup(None), lookup(threads("x", json!(null)))]), None, "FAILED: waits for the earlier machine");
        assert_eq!(pick_linked_thread(&[lookup(threads("x", json!("2026-01-01T00:00:00.000Z"))), lookup(Some(Err(json!(null)))), lookup(threads("y", json!(null)))]), Some(Some("y".into())));
        assert_eq!(pick_linked_thread(&[lookup(Some(Err(json!(null))))]), Some(None));
    }

    #[test]
    fn thread_links_open_the_thread_and_place_the_prompt_like_web() {
        use serde_json::json;
        let link = |url: &str| thread_link(url);
        let thread = |id: &str, prompt: Option<Result<(String, usize), String>>| Some(ThreadLink { thread: id.into(), prompt });
        assert_eq!(link("s5code://threads/env-1/t-1"), thread("t-1", None));
        assert_eq!(link("t3code-dev://threads/env-1/t-1/?prompt=hi"), thread("t-1", Some(Ok(("hi".into(), 0)))));
        assert_eq!(link("https://app.s5code.touchtech.club/threads/env%201/t%3A1"), thread("t:1", None), "FAILED: Copy thread's hosted link does not open in Rune");
        assert_eq!(link("https://app.s5code.touchtech.club/env-1/t-1?prompt=a%E2%80%8Bb"), thread("t-1", Some(Ok(("ab".into(), 1)))));
        assert_eq!(link("https://app.s5code.touchtech.club/env-1/t-1"), None, "a hosted thread route is a link only with a prompt");
        assert!(matches!(link("s5code://threads/env-1/t-1?prompt=%E2%80%8B"), Some(ThreadLink { prompt: Some(Err(_)), .. })));
        for other in ["s5code://threads/env-1", "s5code://threads/a/b/c", "https://example.com/threads/a/b", "s5code://other/a/b"] {
            assert_eq!(link(other), None, "{other}");
        }
        assert!(is_app_link("s5code://threads/env-1/t-1"));

        let mut s = ready(vec![json!({"id": "p1"})], vec![json!({"id": "t1", "projectId": "p1"})]);
        let text = |s: &App| s.ui.chat.composer.drafts.get("t1").map(|d| d.edit.text.clone()).unwrap_or_default();
        assert!(open(&mut s, "s5code://threads/env/t1?prompt=Fix%20it"));
        assert_eq!((s.route.clone(), text(&s)), (Route::Thread("t1".into()), "Fix it".into()));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Prompt added from a link"));
        // `placeLinkedPrompt`: a different draft is replaced only when asked, never appended to.
        assert!(open(&mut s, "s5code://threads/env/t1?prompt=Other%E2%80%8B"));
        assert_eq!((crate::shell::palette::toast_title(&s), text(&s).as_str()), (Some("Linked prompt ready"), "Fix it"), "FAILED: the draft changed before the person chose");
        let warning = ("This prompt had hidden characters, it may be malicious".to_owned(), Some("T3 Code removed 1 hidden character. Read the prompt before you send it.".to_owned()));
        assert!(crate::shell::toast::titles(&s).contains(&warning), "FAILED: no warning about the hidden character");
        crate::shell::palette::press_toast_action(&mut s, "Replace draft");
        assert_eq!(text(&s), "Other");
        let titles: Vec<String> = crate::shell::toast::titles(&s).into_iter().map(|(title, _)| title).collect();
        assert!(titles.contains(&"Prompt added from a link".to_owned()) && !titles.contains(&"Linked prompt ready".to_owned()), "{titles:?}");
        assert!(open(&mut s, "s5code://threads/env/t1?prompt=%E2%80%8B"));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Could not open the linked prompt"));
    }

    #[test]
    fn a_cloned_project_lands_the_draft_or_times_out() {
        let link = |url: &str| parse(url).unwrap().unwrap();
        let awaiting = |url: &str, deadline| Some(Landing::Awaiting { project: "p9".into(), link: link(url), deadline });
        let later = std::time::Instant::now() + CLONE_WAIT;
        let mut s = ready(vec![], vec![]);
        s.ui.links.landing = awaiting("s5code://new?prompt=hi&repo=acme/app", later);
        assert!(!poll_landing(&mut s), "FAILED: keeps waiting for the project");
        assert!(matches!(s.ui.links.landing, Some(Landing::Awaiting { .. })));
        s.ui.links.landing = awaiting("s5code://new?prompt=hi&repo=acme/app", std::time::Instant::now());
        assert!(poll_landing(&mut s));
        assert_eq!((s.ui.links.landing.is_none(), crate::shell::palette::toast_title(&s)), (true, Some("Could not open the link")));
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.projects.push(serde_json::json!({"id": "p9"}));
        }
        s.ui.links.landing = awaiting("s5code://new?pr=acme/app/4", later);
        assert!(poll_landing(&mut s));
        assert_eq!(crate::shell::palette::toast_title(&s), Some("Cloning acme/app"), "FAILED: a pull request link asks to be opened again");
        s.ui.links.landing = awaiting("s5code://new?prompt=hi&repo=acme/app&provider=nobody", later);
        assert!(poll_landing(&mut s));
        assert_eq!(s.route, Route::Home { project_id: Some("p9".into()) });
        let draft = s.ui.chat.composer.drafts.get(&crate::chat::composer::draft_key(&s, s.frame_route())).map(|d| d.edit.text.clone());
        assert_eq!(draft.as_deref(), Some("hi"));
        assert!(crate::shell::palette::toast_description(&s).is_some_and(|d| d.contains("not set up here")), "FAILED: the link's provider is applied on this path too");
    }

    #[test]
    fn a_link_to_a_missing_repository_offers_the_clone() {
        let mut s = App::new();
        let mut state = crate::data::empty_state();
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.ui.links.pending = parse("s5code://new?prompt=hi&repo=acme/app");
        assert!(poll(&mut s));
        let repository = ("acme".to_string(), "app".to_string());
        assert_eq!(s.ui.links.landing, Some(Landing::Missing(NewThread { prompt: "hi".into(), repository: Some(repository.clone()), provider: None, model: None, pull_request: None, hidden: 0 })));
        clone_repository(&mut s);
        assert_eq!(s.ui.links.landing, None, "a machine without newProjectsRoot cannot clone from a link");
        let payload = clone_payload(&repository, "~/code", "p1");
        assert_eq!(
            [&payload["projectId"], &payload["title"], &payload["remoteUrl"], &payload["destinationPath"]].map(|v| v.as_str().unwrap_or_default()),
            ["p1", "app", "https://github.com/acme/app.git", "~/code/app"]
        );
        s.ui.links.pending = parse("s5code://new?prompt=hi");
        assert!(poll(&mut s));
        assert_eq!(s.ui.links.landing, None, "without a Scratch project the link fails instead of drafting elsewhere");
    }

    #[test]
    fn links_clone_where_the_latest_thread_lives() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture(crate::data::empty_state());
        assert_eq!(last_active_environment(&s).as_deref(), Some(crate::env::LOCAL));
        s.ui.desktop.local_off = true;
        assert_eq!(last_active_environment(&s), None, "with this machine off there is no primary environment");
        if let crate::data::Source::Fixture(state) = &mut s.src {
            state.threads = vec![
                serde_json::json!({"id": "t1", "updatedAt": "2026-01-01T00:00:00.000Z"}),
                serde_json::json!({"id": "t2", "updatedAt": "2026-02-01T00:00:00.000Z", "_environment": "box"}),
            ];
        }
        assert_eq!(last_active_environment(&s).as_deref(), Some("box"));
    }

    #[test]
    fn built_links_carry_the_prompt_repo_and_model_and_parse_back() {
        let repo = ("acme".to_string(), "app".to_string());
        let link = new_thread_link("  Fix the tests & ship  ", Some(&repo), Some("claudeAgent"), Some("claude-haiku-5-5")).unwrap();
        assert_eq!(link, "https://app.s5code.touchtech.club/new?prompt=Fix+the+tests+%26+ship&repo=acme%2Fapp&provider=claudeAgent&model=claude-haiku-5-5");
        assert_eq!(
            parse(&link).unwrap().unwrap(),
            NewThread { prompt: "Fix the tests & ship".into(), repository: Some(repo), provider: Some("claudeagent".into()), model: Some("claude-haiku-5-5".into()), pull_request: None, hidden: 0 }
        );
        assert_eq!(new_thread_link("hi", None, None, Some("ignored")).unwrap(), "https://app.s5code.touchtech.club/new?prompt=hi");
        assert_eq!(new_thread_link("   ", None, None, None), None);
        assert_eq!(new_thread_link(&"x".repeat(8_001), None, None, None), None);
        let project = |identity: serde_json::Value| serde_json::json!({ "repositoryIdentity": identity });
        assert_eq!(repository_of(&project(serde_json::json!({"canonicalKey": "github.com/acme/app"}))), Some(("acme".into(), "app".into())));
        assert_eq!(repository_of(&project(serde_json::json!({"canonicalKey": "github.com/fork/app", "origin": {"canonicalKey": "github.com/acme/app"}}))), Some(("acme".into(), "app".into())));
        assert_eq!(repository_of(&project(serde_json::json!({"canonicalKey": "gitlab.com/acme/app"}))), None);
        assert_eq!(repository_of(&serde_json::json!({})), None);
    }
}
