//! Prompt links (packages/shared/src/promptLinks.ts): `s5code://new?prompt=…&repo=owner/name`,
//! or the hosted app's `/new?…`, open a new-thread draft holding the prompt on the repository's
//! project (NewThreadLinkView): with no project for it, they offer to clone it; with no
//! repository, the draft opens in the Scratch project. Nothing is sent; the person reads the
//! prompt in the composer first. They arrive on the command line (Linux and Windows hand links
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
    prompt: String,
    repository: Option<(String, String)>,
}

#[derive(Default)]
pub struct Ui {
    pending: Option<Result<NewThread, String>>,
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
    /// A scratch project is starting for a link without a repository; holds the prompt.
    Scratch(String),
}

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

/// `parsePromptLinkRepository`: `owner/name` or a github.com repository URL.
fn repository(value: &str) -> Option<(String, String)> {
    let value = value.trim();
    let path = ["https://github.com/", "http://github.com/", "https://www.github.com/", "http://www.github.com/"]
        .iter()
        .find_map(|p| value.get(..p.len()).filter(|head| head.eq_ignore_ascii_case(p)).map(|_| value[p.len()..].trim_end_matches('/')))
        .unwrap_or(value);
    let (owner, name) = path.split_once('/')?;
    let name = name.strip_suffix(".git").or_else(|| name.strip_suffix(".GIT")).unwrap_or(name);
    let owner_ok = (1..=39).contains(&owner.len()) && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !owner.starts_with('-');
    let name_ok = (1..=100).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !name.chars().all(|c| c == '.');
    (owner_ok && name_ok).then(|| (owner.to_owned(), name.to_owned()))
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
    parse(url).is_some() || pairing_link(url).is_some() || codex_callback::read_handoff(url).is_some() || resume_route(url).is_some()
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

/// `promptLinkAppPath` + `parseNewThreadLink` for new-thread links. `None` when `url` is not a
/// prompt link at all; thread links and pull-request links are not handled here.
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
    if param("pr").is_some() {
        return Some(Err("Pull request links open in the web app for now.".into()));
    }
    Some((|| {
        let raw = param("prompt").unwrap_or_default();
        if raw.chars().count() > MAX_PROMPT {
            return Err(format!("This prompt is longer than {} characters, so T3 Code did not open it.", "8,000"));
        }
        let (prompt, dropped) = sanitize(&raw);
        if prompt.is_empty() && dropped > 0 {
            return Err("This prompt had only hidden characters, so it may be malicious. T3 Code did not open it.".into());
        }
        if prompt.is_empty() {
            return Err("This link has no prompt.".into());
        }
        let repository = match param("repo") {
            Some(repo) => Some(repository(&repo).ok_or("This link names a repository T3 Code cannot read.")?),
            None => None,
        };
        Ok(NewThread { prompt, repository })
    })())
}

/// Opens a prompt link from anywhere (a second launch, the OS's URL handler), on the UI thread:
/// the draft lands now, or once the shell has loaded. False when `url` is no prompt link.
pub fn open(s: &mut App, url: &str) -> bool {
    if let Some(handoff) = codex_callback::read_handoff(url) {
        codex_callback::handoff(s, handoff);
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
fn project_for(s: &App, (owner, name): &(String, String)) -> Option<String> {
    let path = format!("{owner}/{name}").to_lowercase();
    let keys: Vec<String> = REPOSITORY_HOSTS.iter().map(|h| format!("{h}/{path}")).collect();
    s.state()
        .projects
        .iter()
        .filter(|p| p["archivedAt"].is_null())
        .filter(|p| {
            let identity = &p["repositoryIdentity"];
            let key = identity["origin"]["canonicalKey"].as_str().or(identity["canonicalKey"].as_str()).unwrap_or_default().to_lowercase();
            keys.contains(&key)
        })
        .max_by_key(|p| data::ts(p, "updatedAt").unwrap_or(0))
        .map(|p| data::s(p, "id").to_owned())
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
                .child(crate::kit::Button::new("Add").on_press(confirm_pairing).build(t)),
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
        Some(Landing::Cloning { .. }) => crate::kit::Dialog::new("prompt-link", "Opening the link...")
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
    start_scratch(s, link.prompt);
}

/// `openScratchDraft`: a link without a repository drafts in the Scratch project.
fn start_scratch(s: &mut App, prompt: String) {
    use crate::shell::palette::actions;
    if actions::scratch_available(s) {
        actions::new_without_project(s);
    }
    if actions::scratch_pending(s) {
        s.ui.links.landing = Some(Landing::Scratch(prompt));
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
                    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project) });
                    crate::chat::composer::append_to_draft(s, &link.prompt);
                    true
                }
            }
        }
        Some(Landing::Scratch(prompt)) if actions::scratch_pending(s) => {
            s.ui.links.landing = Some(Landing::Scratch(prompt));
            false
        }
        // The scratch flow reports its own failure.
        Some(Landing::Scratch(prompt)) => {
            let landed = matches!(&s.route, Route::Home { project_id: Some(id) }
                if crate::chat::composer::project_by_id(s, id).is_some_and(|p| actions::is_scratch_project(s, p)));
            if landed {
                crate::chat::composer::append_to_draft(s, &prompt);
            }
            true
        }
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

pub fn poll(s: &mut App) -> bool {
    let paired = poll_pairing(s) | poll_web(s) | poll_landing(s);
    if s.ui.links.pending.is_none() || !s.state().shell_ready {
        return paired;
    }
    match s.ui.links.pending.take().expect("checked above") {
        Err(message) => crate::shell::palette::toast(s, "Could not open the link", Some(&message)),
        Ok(link) => match link.repository.as_ref().map(|repo| project_for(s, repo)) {
            None => start_scratch(s, link.prompt),
            Some(None) => s.ui.links.landing = Some(Landing::Missing(link)),
            Some(Some(id)) => {
                s.navigate(Route::Home { project_id: Some(id) });
                crate::chat::composer::append_to_draft(s, &link.prompt);
            }
        },
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
    s.ui.links.landing = Some(Landing::Missing(NewThread { prompt: "Fix the flaky tests".into(), repository }));
}

pub const SCENES: &[(&str, fn(&mut App))] = &[("link-missing-repository", scene_missing_repository)];

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(web_page("file:///home/a%20b/Index.HTML"), Some(WebPage::File("/home/a b/Index.HTML".into())));
        for page in ["file:///x/page.htm", "file:///x/page.xhtml", "file:///x/page.xhtm"] {
            assert!(matches!(web_page(page), Some(WebPage::File(_))), "{page}");
        }
        for other in ["file:///x/notes.md", "file:///x.html/notes", "file:///x/html", "mailto:a@b.c", "s5code://new?prompt=x", "ftp://a/b.html"] {
            assert_eq!(web_page(other), None, "{other}");
        }
    }

    #[test]
    fn prompt_links_parse_like_the_web() {
        let ok = |url: &str| parse(url).expect("a prompt link").expect("valid");
        let link = ok("t3code://new?prompt=Fix%20the%20tests&repo=pingdotgg/t3code");
        assert_eq!(link, NewThread { prompt: "Fix the tests".into(), repository: Some(("pingdotgg".into(), "t3code".into())) });
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
        assert_eq!(s.ui.links.landing, Some(Landing::Missing(NewThread { prompt: "hi".into(), repository: Some(repository.clone()) })));
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
        assert_eq!(parse(&link).unwrap().unwrap(), NewThread { prompt: "Fix the tests & ship".into(), repository: Some(repo) });
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
