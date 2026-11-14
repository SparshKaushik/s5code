//! Links in timeline Markdown (ChatMarkdown, markdownLinks.ts, threadLinks.ts): file paths read
//! as chips and open in the Files panel (or the editor, outside the workspace), thread links read
//! as chips and open their thread, and everything else opens in the browser.
use crate::data::s;
use crate::theme::{AlphaMul, T};
use crate::{App, Route};
use rune::{LinkPaint, Span, Tx};
use serde_json::json;

#[derive(Default)]
pub struct State {
    /// `shell.openInEditor` requests and the path each opens.
    pending: Vec<(u64, String)>,
}

#[derive(Debug, PartialEq)]
pub enum Target {
    File { path: String, line: Option<usize> },
    Thread(String),
    Web,
}

/// The shared `DEFAULT_HOSTED_APP_URL`, then upstream's, which older messages still carry.
const HOSTED: [&str; 2] = ["https://app.s5code.touchtech.club/threads/", "https://app.t3.codes/threads/"];

/// Basenames that are files without an extension (markdownLinks.ts).
const BARE_FILES: [&str; 26] = [
    "Makefile", "makefile", "GNUmakefile", "Dockerfile", "Containerfile", "Justfile", "justfile", "Rakefile", "Gemfile", "Procfile", "Brewfile", "Caddyfile",
    "Vagrantfile", "Jenkinsfile", "Podfile", "Fastfile", "BUILD", "WORKSPACE", "LICENSE", "LICENCE", "COPYING", "NOTICE", "AUTHORS", "CONTRIBUTORS",
    "CHANGELOG", "README",
];

const POSIX_ROOTS: [&str; 24] = [
    "/Users/", "/home/", "/tmp/", "/var/", "/etc/", "/opt/", "/mnt/", "/Volumes/", "/private/", "/root/", "/usr/", "/bin/", "/sbin/", "/lib/", "/lib64/",
    "/srv/", "/dev/", "/proc/", "/sys/", "/run/", "/boot/", "/media/", "/workspace/", "/workspaces/",
];

pub fn classify(href: &str) -> Target {
    if let Some(id) = thread_id(href) {
        return Target::Thread(id);
    }
    match file(href) {
        Some((path, line)) => Target::File { path, line },
        None => Target::Web,
    }
}

/// `parseThreadLinkHref`: v1 and v2 ids as written, a hosted link's id percent-decoded.
fn thread_id(href: &str) -> Option<String> {
    if let Some(id) = href.strip_prefix("t3-thread://v1/") {
        return (!id.is_empty() && !id.contains('/')).then(|| id.to_owned());
    }
    if let Some(rest) = href.strip_prefix("t3-thread://v2/") {
        let (env, id) = rest.split_once('/')?;
        return (!env.is_empty() && !id.is_empty()).then(|| id.to_owned());
    }
    let rest = HOSTED.iter().find_map(|p| href.strip_prefix(*p))?;
    let (env, id) = rest.split_once('/')?;
    let segment = |v: &str| !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '%' | '-'));
    (segment(env) && segment(id)).then(|| percent_decode(id))
}

fn percent_decode(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(v) = text.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(v);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| text.to_owned())
}

/// `parseMarkdownFileLink`: the path and 1-based line a link names, if it names a local file.
fn file(href: &str) -> Option<(String, Option<usize>)> {
    let mut raw = href.trim().trim_start_matches('<').trim_end_matches('>');
    if raw.is_empty() || raw.starts_with('#') || raw.starts_with("//") || raw.starts_with("www.") {
        return None;
    }
    let file_url = raw.strip_prefix("file://");
    if let Some(rest) = file_url {
        raw = rest;
    }
    let (body, suffix) = raw.find(['?', '#']).map_or((raw, ""), |i| raw.split_at(i));
    let mut path = percent_decode(body);
    if file_url.is_none() && external(&path) {
        return None;
    }
    let mut line = suffix.strip_prefix("#L").and_then(|h| h.split('C').next()).and_then(|n| n.parse().ok());
    if let Some((head, n)) = position(&path) {
        path = head;
        line = line.or(Some(n));
    }
    (file_url.is_some() || looks_like_path(&path)).then_some((path, line))
}

/// A URL scheme other than a drive letter or a `file:12` position.
fn external(path: &str) -> bool {
    let Some((scheme, rest)) = path.split_once(':') else { return false };
    let scheme_ok = scheme.starts_with(|c: char| c.is_ascii_alphabetic()) && scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'));
    if !scheme_ok || (scheme.len() == 1 && rest.starts_with(['\\', '/'])) {
        return false;
    }
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    rest.starts_with("//") || !rest.split(':').all(digits) || rest.split(':').count() > 2
}

/// A trailing `:line` or `:line:column`, split off.
fn position(path: &str) -> Option<(String, usize)> {
    let digits = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit());
    let (head, last) = path.rsplit_once(':')?;
    if !digits(last) || head.is_empty() {
        return None;
    }
    if let Some((file, line)) = head.rsplit_once(':')
        && digits(line)
        && !file.is_empty()
    {
        return Some((file.to_owned(), line.parse().ok()?));
    }
    Some((head.to_owned(), last.parse().ok()?))
}

fn has_extension(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(stem, ext)| {
        !stem.is_empty() && (1..=12).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic())
    })
}

fn looks_like_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    if bytes.len() > 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') {
        return true;
    }
    if ["./", "../", "~/"].iter().any(|p| path.starts_with(*p)) {
        return true;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    let named = has_extension(name) || BARE_FILES.contains(&name);
    if path.starts_with('/') {
        return named || POSIX_ROOTS.iter().any(|r| path.starts_with(r));
    }
    let plain = |c: char| c.is_ascii_alphanumeric() || "._$+@()[]-/".contains(c);
    let spaced = !path.starts_with(' ') && !path.ends_with(' ') && !path.contains("  ");
    if path.contains('/') {
        return spaced && path.chars().all(|c| plain(c) || c == ' ');
    }
    named && !path.contains('@') && path.chars().all(plain)
}

/// A link press: a thread opens if this environment has it, a file opens in the Files panel,
/// and a web link opens where Settings send links.
pub fn open(app: &mut App, url: &str, tx: &mut Tx) {
    match classify(url) {
        Target::Thread(id) => {
            let state = app.state();
            if state.threads.iter().chain(&state.archived).any(|t| s(t, "id") == id) {
                app.navigate(Route::Thread(id));
            } else {
                crate::shell::palette::toast_error(app, "Thread not found", Some("It may belong to another environment."));
            }
        }
        Target::File { path, line } => open_file(app, &path, line),
        // Web's `resolveLinkTarget`: Settings may send links to the preview; Cmd or Ctrl still opens the system browser.
        Target::Web => {
            let mods = tx.mods();
            crate::panel::terminal::open_web(app, tx, url, mods.cmd || mods.ctrl);
        }
    }
}

/// `path` relative to the workspace, when it lies inside it.
fn relative(path: &str, root: Option<&str>) -> Option<String> {
    let root = root.map(|r| r.trim_end_matches('/')).filter(|r| !r.is_empty());
    if let Some(rest) = root.and_then(|r| path.strip_prefix(r)).and_then(|r| r.strip_prefix('/')) {
        return Some(rest.to_owned());
    }
    let absolute = path.starts_with(['/', '~']) || path.as_bytes().get(1) == Some(&b':');
    if absolute || root.is_none() {
        return None;
    }
    let mut rest = path;
    while let Some(r) = rest.strip_prefix("./") {
        rest = r;
    }
    (!rest.starts_with("../")).then(|| rest.to_owned())
}

fn open_file(app: &mut App, path: &str, line: Option<usize>) {
    if let Some(rel) = relative(path, super::workspace_root(app).as_deref()) {
        crate::panel::open_file_at(app, rel, line);
        return;
    }
    let config = app.state().config.as_ref();
    let editor = config
        .and_then(|c| c["availableEditors"].as_array())
        .and_then(|list| list.iter().map(|e| e.as_str().unwrap_or_else(|| s(e, "id"))).find(|e| !e.is_empty() && *e != "file-manager"))
        .map(str::to_owned);
    let Some(editor) = editor else {
        crate::shell::palette::toast_error(app, "Could not open file", Some(path));
        return;
    };
    let target = match line {
        Some(n) => format!("{path}:{n}"),
        None => path.to_owned(),
    };
    if let Some(net) = app.src.net() {
        let id = net.request("shell.openInEditor", json!({"cwd": target, "editor": editor}));
        app.ui.chat.timeline.md_links.pending.push((id, path.to_owned()));
    }
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    for (id, path) in std::mem::take(&mut app.ui.chat.timeline.md_links.pending) {
        match app.src.net().and_then(|net| net.take_reply(id)) {
            None => app.ui.chat.timeline.md_links.pending.push((id, path)),
            Some(reply) => {
                if reply.is_err() {
                    crate::shell::palette::toast_error(app, "Could not open file", Some(&path));
                }
                changed = true;
            }
        }
    }
    changed
}

/// File and thread links read as chips; web links keep the default underline.
pub fn look(t: &T) -> impl Fn(&str) -> Option<(Span, LinkPaint)> + 'static {
    let (ink, rest, hover) = (t.c.foreground, t.c.foreground.alpha_mul(0.08), t.c.foreground.alpha_mul(0.14));
    move |url: &str| match classify(url) {
        Target::Web => None,
        _ => Some((Span::new(0, 0).color(ink).bg(rest, 3.0), LinkPaint::new().bg(hover))),
    }
}

pub fn help(url: &str) -> Option<String> {
    match classify(url) {
        Target::File { path, line: Some(n) } => Some(format!("Open {path}:{n}")),
        Target::File { path, line: None } => Some(format!("Open {path}")),
        Target::Thread(_) => Some("Open thread".into()),
        Target::Web => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_at(path: &str, line: Option<usize>) -> Target {
        Target::File { path: path.into(), line }
    }

    #[test]
    fn thread_links_name_their_thread() {
        assert_eq!(classify("t3-thread://v1/thread:abc"), Target::Thread("thread:abc".into()));
        assert_eq!(classify("t3-thread://v2/env-1/thread:abc"), Target::Thread("thread:abc".into()));
        assert_eq!(classify("https://app.s5code.touchtech.club/threads/env-1/thread%3Aabc"), Target::Thread("thread:abc".into()));
        assert_eq!(classify("https://app.t3.codes/threads/env/t1"), Target::Thread("t1".into()));
        assert_eq!(classify("t3-thread://v2/only-env"), Target::Web);
    }

    #[test]
    fn file_links_follow_the_web_heuristics() {
        assert_eq!(classify("src/main.rs"), file_at("src/main.rs", None));
        assert_eq!(classify("src/main.rs:12"), file_at("src/main.rs", Some(12)));
        assert_eq!(classify("src/main.rs:12:4"), file_at("src/main.rs", Some(12)));
        assert_eq!(classify("/home/me/app/a.ts#L7C2"), file_at("/home/me/app/a.ts", Some(7)));
        assert_eq!(classify("file:///tmp/a%20b.txt"), file_at("/tmp/a b.txt", None));
        assert_eq!(classify("Makefile"), file_at("Makefile", None));
        assert_eq!(classify("./run"), file_at("./run", None));
        assert_eq!(classify("README.md"), file_at("README.md", None));
        for web in ["https://example.com/a.rs", "mailto:a@b.co", "#section", "www.example.com", "user@host.rs", "plainword"] {
            assert_eq!(classify(web), Target::Web, "{web}");
        }
    }

    #[test]
    fn files_inside_the_workspace_open_relative_to_it() {
        let root = Some("/home/me/app/");
        assert_eq!(relative("/home/me/app/src/a.rs", root), Some("src/a.rs".into()));
        assert_eq!(relative("./src/a.rs", root), Some("src/a.rs".into()));
        assert_eq!(relative("/etc/hosts", root), None);
        assert_eq!(relative("../other/a.rs", root), None);
        assert_eq!(relative("src/a.rs", None), None);
    }
}
