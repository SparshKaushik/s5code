//! Prompt links (packages/shared/src/promptLinks.ts): `s5code://new?prompt=…&repo=owner/name`,
//! or the hosted app's `/new?…`, open a new-thread draft holding the prompt on the repository's
//! project. Nothing is sent; the person reads the prompt in the composer first. They arrive on
//! the command line (Linux and Windows hand links to a new process) or through macOS's
//! `on_open_url`, and wait for the shell before they land.

use crate::{App, Route, data};

/// `PROMPT_LINK_MAX_PROMPT_LENGTH`.
const MAX_PROMPT: usize = 8_000;
const HOSTED_APP: &str = "https://app.t3.codes";
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

/// `promptLinkAppPath` + `parseNewThreadLink` for new-thread links. `None` when `url` is not a
/// prompt link at all; thread links and pull-request links are not handled here.
pub fn parse(url: &str) -> Option<Result<NewThread, String>> {
    let parsed = s5_net::url::Url::parse(url.trim()).ok()?;
    // `APP_SCHEME`: this fork's `s5code://` (what the app registers) or upstream's `t3code://`.
    let app_scheme = ["s5code", "t3code"].iter().any(|app| {
        let scheme = parsed.scheme().strip_prefix(app);
        scheme == Some("") || scheme.and_then(|s| s.strip_prefix('-')).is_some_and(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase()))
    });
    let is_new = if app_scheme {
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
    let Some(link) = parse(url) else { return false };
    s.ui.links.pending = Some(link);
    poll(s);
    true
}

/// Links passed on the command line at launch.
pub fn start(s: &mut App) {
    if let Some(url) = std::env::args().skip(1).find(|a| parse(a).is_some()) {
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

/// Lands a waiting link once the shell has loaded projects.
pub fn poll(s: &mut App) -> bool {
    if s.ui.links.pending.is_none() || !s.state().shell_ready {
        return false;
    }
    match s.ui.links.pending.take().expect("checked above") {
        Err(message) => crate::shell::palette::toast(s, "Could not open the link", Some(&message)),
        Ok(link) => {
            let project_id = match &link.repository {
                Some(repo) => match project_for(s, repo) {
                    Some(id) => Some(id),
                    None => {
                        let message = format!("Add {}/{} as a project, then open the link again.", repo.0, repo.1);
                        crate::shell::palette::toast(s, &format!("You don't have {}", repo.1), Some(&message));
                        return true;
                    }
                },
                None => None,
            };
            s.navigate(Route::Home { project_id });
            crate::chat::composer::append_to_draft(s, &link.prompt);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_links_parse_like_the_web() {
        let ok = |url: &str| parse(url).expect("a prompt link").expect("valid");
        let link = ok("t3code://new?prompt=Fix%20the%20tests&repo=pingdotgg/t3code");
        assert_eq!(link, NewThread { prompt: "Fix the tests".into(), repository: Some(("pingdotgg".into(), "t3code".into())) });
        assert_eq!(ok("https://app.t3.codes/new?prompt=hi&repo=https://github.com/a/b.git").repository, Some(("a".into(), "b".into())));
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
        assert!(parse("t3code://pair?pairingUrl=x").is_none());
        assert!(parse("--check-server").is_none());
    }
}
