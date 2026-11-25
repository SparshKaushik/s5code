//! PullRequestMarkdown: a pull request's description or comment, with the pictures and videos it
//! links on lines of their own drawn as media (`splitPullRequestBody`), and a host's private
//! media read through the pull request's environment with the credential it holds for the
//! checkout. Pictures open in chat's lightbox, which the page draws.

use rune::{El, col};

use crate::App;
use crate::chat::timeline::media::{self, Host, Kind};
use crate::data::s as field;
use crate::theme::T;

/// Web's segments also carry an "unknown" attachment for its link card, which the split never
/// produces, so Rune has no card. `name` is a Markdown picture's alt text.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Segment {
    Markdown(String),
    Attachment { url: String, name: String, video: bool },
}

/// How far a `<video>` tag may reach for its closing tag.
const VIDEO_TAG_MAX_LINES: usize = 8;
const VIDEO_EXTENSIONS: [&str; 5] = ["mp4", "webm", "mov", "m4v", "ogv"];
const IMAGE_EXTENSIONS: [&str; 9] = ["png", "jpg", "jpeg", "gif", "webp", "avif", "heic", "heif", "svg"];

/// The body drawn: Markdown as Markdown, and its media read as web's PullRequestMarkdown reads
/// them, at the detail's workspace root on the pull request's own environment.
pub(super) fn view(s: &App, t: &T, body: &str) -> El {
    let segments = segments(body);
    if !segments.iter().any(|seg| matches!(seg, Segment::Attachment { .. })) {
        return super::pr_detail::markdown(t, body);
    }
    let (env, cwd) = scope(s);
    let fetches = fetches(&segments);
    let gallery: Vec<String> = segments
        .iter()
        .zip(&fetches)
        .filter_map(|(seg, fetch)| match seg {
            Segment::Attachment { url, video: false, .. } => Some(media::linked_key(url, host(env.as_deref(), cwd, fetch.as_deref()).as_ref())),
            _ => None,
        })
        .collect();
    let mut out = col().w_full().gap(12.0);
    for (seg, fetch) in segments.iter().zip(&fetches) {
        out = out.child(match seg {
            Segment::Markdown(text) => super::pr_detail::markdown(t, text),
            Segment::Attachment { url, name, video } => media::linked(s, url, name, if *video { Kind::Video } else { Kind::Image }, host(env.as_deref(), cwd, fetch.as_deref()).as_ref(), &gallery, t),
        });
    }
    out
}

fn host<'a>(env: Option<&'a str>, cwd: Option<&'a str>, fetch: Option<&'a str>) -> Option<Host<'a>> {
    Some(Host { env: env?, cwd: cwd?, fetch: fetch? })
}

/// The pull request's own environment, and its detail's workspace root.
fn scope(s: &App) -> (Option<String>, Option<&str>) {
    let u = &s.ui.pages.detail;
    let cwd = [u.detail.as_ref(), u.entry.as_ref()].into_iter().flatten().map(|d| field(d, "workspaceRoot")).find(|r| !r.is_empty());
    (u.entry.as_ref().map(|e| crate::env::of(s, e)), cwd)
}

fn fetches(segments: &[Segment]) -> Vec<Option<String>> {
    segments
        .iter()
        .map(|seg| match seg {
            Segment::Attachment { url, .. } => host_media(url).map(|m| m.fetch_url),
            Segment::Markdown(_) => None,
        })
        .collect()
}

/// Scenes: the description's media drawn as loaded test pictures, and its videos as failed
/// (the sandbox, like every platform but macOS, can't play them). `open` opens the first picture.
pub(super) fn scene(s: &mut App, open: bool) {
    let body = s.ui.pages.detail.detail.as_ref().map(|d| field(d, "body").to_owned()).unwrap_or_default();
    let segments = segments(&body);
    let (env, cwd) = scope(s);
    let cwd = cwd.map(str::to_owned);
    let mut gallery = Vec::new();
    for (seg, fetch) in segments.iter().zip(fetches(&segments)) {
        let Segment::Attachment { url, name, video } = seg else { continue };
        let key = media::linked_key(url, host(env.as_deref(), cwd.as_deref(), fetch.as_deref()).as_ref());
        media::scene_linked(s, &key, name, if *video { Kind::Video } else { Kind::Image }, *video);
        if !*video {
            gallery.push(key);
        }
    }
    if open {
        media::scene_open(s, gallery);
    }
}

/// `split`, then lines of nothing but `![alt](src)` web pictures or videos drawn as them, as
/// ChatMarkdown draws a standalone image (with the host's credential when it keeps them).
pub(super) fn segments(body: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    for seg in split(body) {
        let Segment::Markdown(text) = seg else {
            out.push(seg);
            continue;
        };
        let mut prose: Vec<&str> = Vec::new();
        let mut open_fence = None;
        for line in text.split('\n') {
            let found = (!code(&mut open_fence, line)).then(|| pictures(line)).flatten();
            match found {
                Some(found) => {
                    out.extend(flush(&mut prose));
                    out.extend(found);
                }
                None => prose.push(line),
            }
        }
        out.extend(flush(&mut prose));
    }
    out
}

fn pictures(line: &str) -> Option<Vec<Segment>> {
    media::images(line)?
        .into_iter()
        .map(|(alt, src)| {
            let kind = media::kind(&src);
            (web_url(&src) && kind != Some("audio")).then(|| Segment::Attachment { video: kind == Some("video"), name: alt, url: src })
        })
        .collect()
}

/// A run of Markdown lines, without the blank lines around it. Never leading spaces: four of
/// them open indented code.
fn flush(lines: &mut Vec<&str>) -> Option<Segment> {
    let text = lines.join("\n");
    lines.clear();
    let text = text.trim_start_matches('\n').trim_end();
    (!text.trim().is_empty()).then(|| Segment::Markdown(text.to_owned()))
}

/// Whether `line` is code, or a fence opening or closing it.
fn code(open_fence: &mut Option<(char, usize)>, line: &str) -> bool {
    if let Some((marker, len, info)) = fence(line) {
        match *open_fence {
            None => *open_fence = Some((marker, len)),
            // Closes only on the same marker, at least as long, with nothing after it.
            Some((open, open_len)) if marker == open && len >= open_len && info.trim().is_empty() => *open_fence = None,
            Some(_) => {}
        }
        return true;
    }
    open_fence.is_some() || line.starts_with("    ") || line.starts_with('\t')
}

/// `splitPullRequestBody`: lines that are only a picture's or video's URL, or a `<video>` tag,
/// become attachments; fenced and indented code is left alone.
pub(super) fn split(body: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut markdown: Vec<&str> = Vec::new();
    let mut open_fence: Option<(char, usize)> = None;
    let lines: Vec<&str> = body.split('\n').collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if code(&mut open_fence, line) {
            markdown.push(line);
            continue;
        }
        if let Some(found) = attachment(line) {
            segments.extend(flush(&mut markdown));
            segments.push(found);
            continue;
        }
        if !standalone_video_tag(line) {
            markdown.push(line);
            continue;
        }
        let first = index - 1;
        let last = (first + VIDEO_TAG_MAX_LINES).min(lines.len()) - 1;
        let mut cursor = first;
        while cursor < last && !video_tag_end(lines[cursor]) {
            cursor += 1;
        }
        let source = video_tag_end(lines[cursor]).then(|| video_tag_src(&lines[first..=cursor].join("\n"))).flatten();
        match source.filter(|src| web_url(src)) {
            Some(url) => {
                segments.extend(flush(&mut markdown));
                segments.push(Segment::Attachment { url, name: String::new(), video: true });
                index = cursor + 1;
            }
            // Unclosed, or nothing linkable: only this line is prose, the rest parse on their own.
            None => markdown.push(line),
        }
    }
    segments.extend(flush(&mut markdown));
    segments
}

/// A code fence: its marker, its length, and what follows it. Up to three spaces may lead.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let rest = line.trim_start();
    if line[..line.len() - rest.len()].chars().count() > 3 {
        return None;
    }
    let marker = rest.chars().next().filter(|c| matches!(*c, '`' | '~'))?;
    let len = rest.chars().take_while(|c| *c == marker).count();
    (len >= 3).then(|| (marker, len, &rest[len..]))
}

/// `attachmentFromLine`: a bare URL or a lone `[text](url)` naming a video or a picture.
fn attachment(line: &str) -> Option<Segment> {
    let line = line.trim();
    let url = bare_url(line).or_else(|| markdown_link(line))?;
    if !web_url(url) {
        return None;
    }
    if has_extension(url, &VIDEO_EXTENSIONS) || host_media(url).is_some_and(|m| m.video) {
        return Some(Segment::Attachment { url: url.to_owned(), name: String::new(), video: true });
    }
    has_extension(url, &IMAGE_EXTENSIONS).then(|| Segment::Attachment { url: url.to_owned(), name: String::new(), video: false })
}

fn scheme_rest(url: &str) -> Option<&str> {
    url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))
}

/// `^<?(https?://\S+?)>?$`
fn bare_url(line: &str) -> Option<&str> {
    if line.chars().any(char::is_whitespace) {
        return None;
    }
    let url = line.strip_prefix('<').unwrap_or(line);
    let rest = scheme_rest(url)?;
    let url = if rest.len() > 1 { url.strip_suffix('>').unwrap_or(url) } else { url };
    (!scheme_rest(url)?.is_empty()).then_some(url)
}

/// `^\[[^\]]*\]\((https?://[^\s)]+)\)$`
fn markdown_link(line: &str) -> Option<&str> {
    let (_, rest) = line.strip_prefix('[')?.split_once(']')?;
    let url = rest.strip_prefix('(')?.strip_suffix(')')?;
    let tail = scheme_rest(url)?;
    (!tail.is_empty() && !tail.chars().any(|c| c.is_whitespace() || c == ')')).then_some(url)
}

/// `new URL(url)` reads it, as http or https, with a host.
fn web_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    scheme_rest(&lower).is_some_and(|rest| rest.split(['/', '?', '#']).next().is_some_and(|host| !host.is_empty()))
}

/// The URL's path ends in one of `extensions` (any case), before a query or fragment.
fn has_extension(url: &str, extensions: &[&str]) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.match_indices('.').any(|(at, _)| {
        let after = &lower[at + 1..];
        extensions.iter().any(|ext| after.strip_prefix(*ext).is_some_and(|tail| tail.is_empty() || tail.starts_with(['?', '#'])))
    })
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `^\s*<video\b`, any case.
fn standalone_video_tag(line: &str) -> bool {
    let lower = line.trim_start().to_ascii_lowercase();
    lower.strip_prefix("<video").is_some_and(|rest| !rest.starts_with(word))
}

/// `</video>\s*$`, any case.
fn video_tag_end(line: &str) -> bool {
    line.trim_end().to_ascii_lowercase().ends_with("</video>")
}

/// `<(?:video|source)\b[^>]*\bsrc\s*=\s*["']([^"']+)["']`, any case: like the greedy pattern, the
/// last `src` before the tag's `>` wins.
fn video_tag_src(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    for (open, _) in lower.match_indices('<') {
        let after = &lower[open + 1..];
        let Some(name) = ["video", "source"].into_iter().find(|n| after.strip_prefix(*n).is_some_and(|r| !r.starts_with(word))) else { continue };
        let start = open + 1 + name.len();
        let end = lower[start..].find('>').map_or(lower.len(), |at| start + at);
        let found = lower[start..end].match_indices("src").map(|(at, _)| start + at).filter(|at| !lower[..*at].ends_with(word)).collect::<Vec<_>>();
        for at in found.into_iter().rev() {
            let rest = html[at + 3..].trim_start().strip_prefix('=').map(str::trim_start);
            let Some(value) = rest.and_then(|r| r.strip_prefix(['"', '\''])) else { continue };
            let Some(close) = value.find(['"', '\'']) else { continue };
            if close > 0 {
                return Some(value[..close].to_owned());
            }
        }
    }
    None
}

#[derive(Debug, PartialEq)]
pub(super) struct HostMedia {
    pub fetch_url: String,
    pub video: bool,
}

/// `gitHubMediaUrl`: GitHub serves uploads, raw files and LFS objects to a credentialed request.
pub(super) fn host_media(url: &str) -> Option<HostMedia> {
    let rest = url.get(..8).filter(|s| s.eq_ignore_ascii_case("https://")).map(|_| &url[8..])?;
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    let tail = &rest[authority_end..];
    let path_end = tail.find(['?', '#']).unwrap_or(tail.len());
    let path = if path_end == 0 { "/" } else { &tail[..path_end] };
    let search = tail[path_end..].split('#').next().unwrap_or("");
    let search = if search == "?" { "" } else { search };
    let file = |fetch_url: String| Some(HostMedia { fetch_url, video: false });
    match host.as_str() {
        "raw.githubusercontent.com" | "media.githubusercontent.com" => return file(format!("https://{host}{path}{search}")),
        "github.com" | "www.github.com" => {}
        _ => return None,
    }
    let parts: Vec<&str> = path.split('/').skip(1).collect();
    fn id(s: &str) -> bool {
        !s.is_empty() && s.chars().all(|c| word(c) || c == '-')
    }
    match parts.as_slice() {
        ["user-attachments", "assets", asset] if id(asset) => Some(HostMedia { fetch_url: format!("https://github.com{path}"), video: true }),
        [owner, repo, "assets", number, asset] if !owner.is_empty() && !repo.is_empty() && !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()) && id(asset) => {
            file(format!("https://github.com{path}"))
        }
        [owner, repo, "raw" | "blob", file_path @ ..] if !owner.is_empty() && !repo.is_empty() && !file_path.is_empty() && !path.ends_with('/') => {
            file(format!("https://raw.githubusercontent.com/{owner}/{repo}/{}", file_path.join("/")))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(url: &str) -> Segment {
        Segment::Attachment { url: url.into(), name: String::new(), video: true }
    }

    fn picture(url: &str, name: &str) -> Segment {
        Segment::Attachment { url: url.into(), name: name.into(), video: false }
    }

    fn md(text: &str) -> Segment {
        Segment::Markdown(text.into())
    }

    #[test]
    fn lines_of_media_split_out_of_the_body() {
        let upload = "https://github.com/user-attachments/assets/0f1e-2d3c";
        let body = format!("## Demo\n\n{upload}\n\nAfter.\n[shot](https://x.dev/a.PNG?v=1)\n<https://x.dev/clip.webm>\nhttps://x.dev/page");
        assert_eq!(
            split(&body),
            vec![md("## Demo"), video(upload), md("After."), picture("https://x.dev/a.PNG?v=1", ""), video("https://x.dev/clip.webm"), md("https://x.dev/page")]
        );
        assert_eq!(split("  \n\n"), vec![]);
    }

    #[test]
    fn lines_of_pictures_draw_as_them() {
        let body = "Before\n![shot](https://github.com/user-attachments/assets/ab-1) ![](<https://x.dev/b.png>)\nAfter ![inline](https://x.dev/c.png)\n![clip](https://x.dev/d.MP4)\n![song](https://x.dev/e.mp3)\n![local](docs/f.png)\n```\n![code](https://x.dev/g.png)\n```";
        assert_eq!(
            segments(body),
            vec![
                md("Before"),
                picture("https://github.com/user-attachments/assets/ab-1", "shot"),
                picture("https://x.dev/b.png", ""),
                md("After ![inline](https://x.dev/c.png)"),
                Segment::Attachment { url: "https://x.dev/d.MP4".into(), name: "clip".into(), video: true },
                md("![song](https://x.dev/e.mp3)\n![local](docs/f.png)\n```\n![code](https://x.dev/g.png)\n```"),
            ]
        );
        assert_eq!(segments("https://x.dev/a.mp4"), split("https://x.dev/a.mp4"));
    }

    #[test]
    fn code_keeps_its_links() {
        let body = "```\nhttps://x.dev/a.mp4\n~~~\n```ts\nhttps://x.dev/b.mp4\n```\n    https://x.dev/c.mp4\nhttps://x.dev/d.mp4";
        assert_eq!(split(body), vec![md("```\nhttps://x.dev/a.mp4\n~~~\n```ts\nhttps://x.dev/b.mp4\n```\n    https://x.dev/c.mp4"), video("https://x.dev/d.mp4")]);
        assert_eq!(split("ftp://x.dev/a.mp4\n[a](https://x.dev/a b.mp4)"), vec![md("ftp://x.dev/a.mp4\n[a](https://x.dev/a b.mp4)")]);
    }

    #[test]
    fn a_video_tag_is_one_attachment_when_it_closes() {
        let tag = "<video controls>\n  <source src=\"https://x.dev/v\" type=\"video/mp4\">\n</video>";
        assert_eq!(split(&format!("Intro\n{tag}\nOutro")), vec![md("Intro"), video("https://x.dev/v"), md("Outro")]);
        assert_eq!(split("<VIDEO SRC='https://x.dev/w'></VIDEO>"), vec![video("https://x.dev/w")]);
        let unclosed = "<video src=\"https://x.dev/v\">\nhttps://x.dev/a.mp4";
        assert_eq!(split(unclosed), vec![md("<video src=\"https://x.dev/v\">"), video("https://x.dev/a.mp4")]);
        assert_eq!(split("<video src=\"/local.mp4\"></video>"), vec![md("<video src=\"/local.mp4\"></video>")]);
        assert_eq!(split("<videos src=\"https://x.dev/v\"></video>"), vec![md("<videos src=\"https://x.dev/v\"></video>")]);
    }

    #[test]
    fn github_media_goes_through_the_raw_host() {
        let media = |url: &str| host_media(url).map(|m| (m.fetch_url, m.video));
        assert_eq!(media("https://www.github.com/user-attachments/assets/ab-12?x=1"), Some(("https://github.com/user-attachments/assets/ab-12".into(), true)));
        assert_eq!(media("https://github.com/o/r/assets/123/ab-12"), Some(("https://github.com/o/r/assets/123/ab-12".into(), false)));
        assert_eq!(media("https://github.com/o/r/blob/main/docs/a.png"), Some(("https://raw.githubusercontent.com/o/r/main/docs/a.png".into(), false)));
        assert_eq!(media("https://RAW.githubusercontent.com/o/r/main/a.png?t=1#f"), Some(("https://raw.githubusercontent.com/o/r/main/a.png?t=1".into(), false)));
        assert_eq!(media("http://github.com/user-attachments/assets/ab"), None);
        assert_eq!(media("https://github.com/o/r/blob/main/"), None);
        assert_eq!(media("https://github.com/o/r/pull/1"), None);
        assert_eq!(media("https://gitlab.com/user-attachments/assets/ab"), None);
    }
}
