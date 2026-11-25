//! Pictures and videos in chat (ChatMarkdownMedia, MediaActions, MediaVideoPlayer,
//! OpenMediaLink): a Markdown paragraph of images draws them, right-click offers web's media
//! actions, and a video plays inline or, where Rune cannot play it, shows web's unavailable card.
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons, kit};
use rune::{Color, El, Pixels, Player, PlayerState, PointerPress, SurfaceFit, Tx, col, el, icon, row, text};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const IMAGE_EXTENSIONS: [&str; 13] = ["avif", "bmp", "gif", "ico", "heic", "heif", "jpeg", "jpg", "png", "svg", "tif", "tiff", "webp"];
const VIDEO_EXTENSIONS: [&str; 7] = ["avi", "m4v", "mkv", "mov", "mp4", "ogv", "webm"];
const AUDIO_EXTENSIONS: [&str; 11] = ["mp3", "wav", "ogg", "oga", "flac", "aac", "m4a", "opus", "aiff", "webm-audio", "weba"];
/// Media read into memory (a picture, or a data URL) stops here.
const MAX_BYTES: u64 = 20 * 1024 * 1024;
/// 30rem: Markdown media never draws wider or taller.
const MAX_SIDE: f32 = 480.0;
/// Pictures decode to the size they draw at on a 2x display, never their originals: Markdown
/// media at 30rem. The original bytes are dropped once decoded; Save and Copy image read the
/// source again, as web does.
pub const MARKDOWN_DECODE: u32 = 960;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Image,
    Video,
    Audio,
}

impl Kind {
    fn noun(self) -> &'static str {
        match self {
            Kind::Image => "image",
            Kind::Video => "video",
            Kind::Audio => "audio",
        }
    }
}

/// `MediaReference`: where the media lives, for the copy actions and the tooltip.
#[derive(Clone, Debug, PartialEq)]
pub enum Reference {
    File { path: String, relative: Option<String> },
    Url(String),
}

/// `MediaActionSource`: what the actions act on. `name` is the Markdown alt text or file name.
#[derive(Clone, Debug, PartialEq)]
pub struct Media {
    pub kind: Kind,
    pub name: String,
    pub reference: Option<Reference>,
    /// The workspace-relative path "Open in file viewer" opens.
    pub open_file: Option<String>,
}

impl Media {
    fn display_name(&self) -> String {
        if self.name.trim().is_empty() { self.kind.noun().to_owned() } else { self.name.clone() }
    }
}

/// A source whose URL is known: its picture decoding off-thread, or its video player.
#[derive(Clone)]
pub struct Loaded {
    pub url: String,
    pub name: String,
    pub kind: Option<Kind>,
    pub pixels: Pixels,
    broken: Arc<AtomicBool>,
    player: Option<Player>,
}

impl Loaded {
    /// `side`: the longest side a picture decodes to.
    pub fn new(url: String, name: String, kind: Option<Kind>, side: u32, waker: Option<rune::Waker>) -> Self {
        let mut loaded = Self { url, name, kind, pixels: Pixels::new(), broken: Arc::default(), player: None };
        match kind {
            Some(Kind::Image) => {
                let (url, pixels, broken) = (loaded.url.clone(), loaded.pixels.clone(), loaded.broken.clone());
                std::thread::spawn(move || {
                    match fetch(&url).and_then(|data| decode(&data, side)) {
                        Ok(image) => pixels.set(image.width(), image.height(), image.into_raw()),
                        Err(_) => broken.store(true, Ordering::Relaxed),
                    }
                    if let Some(wake) = &waker {
                        wake();
                    }
                });
            }
            Some(Kind::Video) => {
                let player = Player::url(loaded.url.clone());
                if let Some(wake) = waker {
                    player.set_waker(wake);
                }
                loaded.player = Some(player);
            }
            Some(Kind::Audio) => {
                let player = Player::audio(loaded.url.clone());
                if let Some(wake) = waker {
                    player.set_waker(wake);
                }
                // Its length shows before it plays, as web's preload="metadata".
                player.preload();
                loaded.player = Some(player);
            }
            None => {}
        }
        loaded
    }

    pub fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    /// The picture could not be read, or the video cannot play here.
    pub fn failed(&self) -> bool {
        self.broken.load(Ordering::Relaxed) || self.player.as_ref().is_some_and(|p| matches!(p.state(), PlayerState::Failed(_)))
    }


    /// What a memoized view of this media reads, so it rebuilds when it changes.
    fn stamp(&self) -> String {
        let player = self.player.as_ref().map(|p| format!("{:?}{}{:?}", p.state(), p.is_playing(), p.natural_size()));
        format!("{}:{:?}:{player:?}", self.failed(), self.pixels.size())
    }
}

fn decode(data: &[u8], side: u32) -> Result<image::RgbaImage, String> {
    let image = image::load_from_memory(data).map_err(|e| e.to_string())?;
    let image = if image.width() > side || image.height() > side { image.thumbnail(side, side) } else { image };
    Ok(image.to_rgba8())
}

/// `readMediaPng`: the source read again, as PNG for the clipboard.
fn read_png(url: &str) -> Result<Vec<u8>, String> {
    let bytes = fetch(url)?;
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(bytes);
    }
    let image = image::load_from_memory(&bytes).map_err(|_| "The image could not be decoded for copying. Try saving it instead.".to_owned())?;
    if image.width() == 0 || image.height() == 0 || u64::from(image.width()) * u64::from(image.height()) > 64_000_000 {
        return Err("This image is too large or has no usable dimensions. Try saving it instead.".to_owned());
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).map_err(|_| "The image could not be converted to PNG.".to_owned())?;
    Ok(out.into_inner())
}

/// `safeDecodeURIComponent`'s bytes: malformed escapes stay as written.
fn percent_bytes(v: &str) -> Vec<u8> {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && let Some(byte) = v.get(i + 1..i + 3).filter(|h| h.bytes().all(|c| c.is_ascii_hexdigit())).and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn percent_decode(v: &str) -> String {
    String::from_utf8(percent_bytes(v)).unwrap_or_else(|_| v.to_owned())
}

/// A data URL's payload, or the body at an http(s) URL.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    if url.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:")) {
        use base64::Engine;
        let (meta, payload) = url[5..].split_once(',').ok_or("This inline media could not be decoded.")?;
        let payload = payload.split('#').next().unwrap_or("");
        return if meta.split(';').any(|p| p.trim().eq_ignore_ascii_case("base64")) {
            base64::engine::general_purpose::STANDARD.decode(percent_decode(payload)).map_err(|_| "This inline media could not be decoded.".to_owned())
        } else {
            Ok(percent_bytes(payload))
        };
    }
    use std::io::Read;
    let response = get(url)?;
    let mut data = Vec::new();
    response.into_reader().take(MAX_BYTES).read_to_end(&mut data).map_err(|e| e.to_string())?;
    Ok(data)
}

fn request(url: &str) -> Result<ureq::Response, String> {
    ureq::get(url).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("The file could not be fetched (HTTP {code})."),
        ureq::Error::Transport(_) => "The file could not be fetched. Check the connection and try again.".to_owned(),
    })
}

/// `fetchMediaBlob`: the response at an http(s) URL, or web's reason it can't be read.
fn get(url: &str) -> Result<ureq::Response, String> {
    let response = request(url)?;
    if response.content_type() == "text/html" {
        return Err("This link returned a web page instead of media. Open the original URL.".to_owned());
    }
    Ok(response)
}

/// `downloadMedia`, to a path the save dialog picked; a video streams to disk.
pub(crate) fn download(url: &str, path: &std::path::Path) -> Result<(), String> {
    if url.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:")) {
        return std::fs::write(path, fetch(url)?).map_err(|e| e.to_string());
    }
    let response = get(url)?;
    let mut file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut response.into_reader(), &mut file).map(drop).map_err(|e| e.to_string())
}

/// `mediaKindFromPath`: "image", "video" or "audio" by the path's or URL's extension.
pub fn kind(path: &str) -> Option<&'static str> {
    let source = path.trim();
    let source = source.strip_prefix('<').and_then(|v| v.strip_suffix('>')).unwrap_or(source);
    let lower = source.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("data:") {
        let mime = rest.split([';', ',']).next().unwrap_or("");
        return ["image", "video", "audio"].into_iter().find(|k| mime.strip_prefix(*k).and_then(|m| m.strip_prefix('/')).is_some_and(|m| !m.is_empty()));
    }
    let mut path = source.split(['?', '#']).next().unwrap_or("");
    if ["http:", "https:", "file:"].iter().any(|p| lower.starts_with(*p)) || source.starts_with("//") {
        let after = source.split_once("//").map_or(source, |(_, rest)| rest);
        path = after.find('/').map_or("", |i| &after[i..]).split(['?', '#']).next().unwrap_or("");
    }
    let decoded = percent_decode(path);
    let name = decoded.rsplit(['/', '\\']).next().unwrap_or("");
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    let ext = ext.as_str();
    if IMAGE_EXTENSIONS.contains(&ext) {
        Some("image")
    } else if VIDEO_EXTENSIONS.contains(&ext) {
        Some("video")
    } else if AUDIO_EXTENSIONS.contains(&ext) {
        Some("audio")
    } else {
        None
    }
}

/// A sent attachment's kind: images by MIME type, videos by MIME type or extension.
pub fn attachment_kind(mime: &str, name: &str) -> Option<Kind> {
    let mime = mime.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    if mime.starts_with("image/") {
        return Some(Kind::Image);
    }
    let generic = mime.is_empty() || mime == "application/octet-stream";
    if mime.starts_with("video/") || (generic && kind(name) == Some("video")) {
        return Some(Kind::Video);
    }
    (mime.starts_with("audio/") || (generic && kind(name) == Some("audio"))).then_some(Kind::Audio)
}

fn windows_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\')) || path.starts_with("\\\\")
}

/// `classifyMarkdownImageSource`: a URL the client can load, or a host file to sign first.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Direct(String),
    File(String),
}

pub fn classify(src: &str, root: Option<&str>) -> Option<Source> {
    let source = src.trim();
    let source = source.strip_prefix('<').and_then(|v| v.strip_suffix('>')).unwrap_or(source);
    if source.is_empty() || source.starts_with(['#', '?']) {
        return None;
    }
    let lower = source.to_ascii_lowercase();
    if ["http:", "https:", "data:"].iter().any(|p| lower.starts_with(*p)) || source.starts_with("//") {
        return Some(Source::Direct(source.to_owned()));
    }
    let file = lower.starts_with("file:");
    let raw = if file {
        let rest = &source[5..];
        rest.strip_prefix("//").map_or(rest, |r| &r[r.find('/').unwrap_or(r.len())..])
    } else {
        source
    };
    let mut path = percent_decode(raw.split(['?', '#']).next().unwrap_or(""));
    // `/C:/x` from a file URL is `C:/x`.
    if path.starts_with('/') && windows_absolute(&path[1..]) {
        path.remove(0);
    }
    if path.is_empty() {
        return None;
    }
    let host_file = |path: String| {
        let network = path.starts_with("//") || path.starts_with("\\\\");
        let inside = matches!(file_reference(&path, root), Reference::File { relative: Some(_), .. });
        (!network || inside).then_some(Source::File(path))
    };
    if file || path.starts_with('/') || windows_absolute(&path) {
        return host_file(path);
    }
    let scheme = path.split_once(':').is_some_and(|(s, _)| s.starts_with(|c: char| c.is_ascii_alphabetic()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')));
    if scheme || path.starts_with("~/") || path.starts_with("~\\") {
        return None;
    }
    let root = root?;
    let separator = if windows_absolute(root) { "\\" } else { "/" };
    let relative = path.replace(['/', '\\'], separator);
    Some(Source::File(format!("{}{separator}{}", root.trim_end_matches(['/', '\\']), relative.trim_start_matches(['/', '\\']))))
}

/// An absolute path's root and normalized segments, and whether it is a Windows path.
fn segments(path: &str) -> Option<(String, Vec<String>, bool)> {
    let windows = windows_absolute(path);
    let normalized = if windows { path.replace('\\', "/") } else { path.to_owned() };
    let prefix = if windows && normalized.as_bytes().get(1) == Some(&b':') { 3 } else if !windows && normalized.starts_with('/') { 1 } else { return None };
    let mut parts: Vec<String> = Vec::new();
    for part in normalized[prefix..].split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p.to_owned()),
        }
    }
    let root = normalized[..prefix].trim_end_matches('/');
    Some((if windows { root.to_lowercase() } else { root.to_owned() }, parts, windows))
}

/// `mediaFileReference`: the path, and its workspace-relative form when it lies inside.
pub fn file_reference(path: &str, root: Option<&str>) -> Reference {
    let relative = root.and_then(|root| {
        let (target_root, target, windows) = segments(path)?;
        let (workspace_root, workspace, workspace_windows) = segments(root)?;
        let same = |a: &String, b: &String| if windows { a.eq_ignore_ascii_case(b) } else { a == b };
        let inside = windows == workspace_windows && target_root == workspace_root && target.len() > workspace.len() && workspace.iter().zip(&target).all(|(a, b)| same(a, b));
        inside.then(|| target[workspace.len()..].join("/"))
    });
    Reference::File { path: path.to_owned(), relative }
}

/// `mediaUrlReference`: an http(s) URL as authored.
pub fn url_reference(url: &str) -> Option<Reference> {
    let lower = url.to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://") || url.starts_with("//")).then(|| Reference::Url(url.to_owned()))
}

/// `mediaReferenceFileName`.
pub fn file_name(reference: &Reference) -> Option<String> {
    let name = match reference {
        Reference::File { path, .. } if windows_absolute(path) => path.rsplit(['/', '\\']).next(),
        Reference::File { path, .. } => path.rsplit('/').next(),
        Reference::Url(url) => {
            let after = url.split_once("//").map_or(url.as_str(), |(_, rest)| rest);
            let path = after.find('/').map_or("", |i| &after[i..]).split(['?', '#']).next().unwrap_or("");
            return path.rsplit('/').next().filter(|n| !n.is_empty()).map(percent_decode);
        }
    };
    name.filter(|n| !n.is_empty()).map(str::to_owned)
}

/// `formatFilePathForClipboard`: a Windows host's paths keep backslashes.
fn clipboard_path(text: &str, host_path: &str) -> String {
    if windows_absolute(host_path) { text.replace('/', "\\") } else { text.to_owned() }
}

/// `OpenMediaLink`: its label and URL, for web URLs only.
pub fn open_link(original: Option<&str>, src: Option<&str>) -> Option<(&'static str, String)> {
    let web = |u: &str| {
        let lower = u.to_ascii_lowercase();
        lower.starts_with("http://") || lower.starts_with("https://") || u.starts_with("//")
    };
    let original = original.filter(|u| web(*u));
    let url = original.or(src)?;
    let url = if url.starts_with("//") { format!("https:{url}") } else { url.to_owned() };
    web(url.as_str()).then(|| (if original.is_some() { "Open original" } else { "Open in browser" }, url))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    CopyFullPath,
    CopyRelativePath,
    CopyUrl,
    OpenFile,
    Save,
    CopyImage,
}

/// MediaActions' menu: each item's action, label and whether it is disabled.
pub fn actions(media: &Media, available: bool) -> Vec<(Action, String, bool)> {
    let mut items = Vec::new();
    match &media.reference {
        Some(Reference::File { relative, .. }) => {
            items.push((Action::CopyFullPath, "Copy full path".to_owned(), false));
            if relative.is_some() {
                items.push((Action::CopyRelativePath, "Copy relative path".to_owned(), false));
            }
        }
        Some(Reference::Url(_)) => items.push((Action::CopyUrl, "Copy URL".to_owned(), false)),
        None => {}
    }
    if media.open_file.is_some() {
        items.push((Action::OpenFile, "Open in file viewer".to_owned(), false));
    }
    items.push((Action::Save, format!("Save {}", media.kind.noun()), !available));
    if media.kind == Kind::Image {
        items.push((Action::CopyImage, "Copy image".to_owned(), !available));
    }
    items
}

/// An asset is signed by `env`'s server (the active one when `None`); a pull request's host media
/// falls back to its own URL when that fails or can't be asked, as web's assets do.
#[derive(Clone)]
enum Access {
    Direct(String),
    Asset { resource: Value, env: Option<String>, fallback: Option<String> },
}

struct Entry {
    media: Media,
    request: Option<(u64, Option<String>, Option<String>)>,
    loaded: Option<Loaded>,
    failed: bool,
}

#[derive(Default)]
pub struct State {
    entries: BTreeMap<String, Entry>,
    /// Media a view drew without an entry yet; `poll` starts loading them.
    wanted: RefCell<Vec<(String, Media, Access)>>,
    /// The media whose actions menu is open, and where the right-click was.
    pub menu: Option<(String, f32, f32)>,
    /// Failures from save threads, to toast: title and description.
    errors: Arc<Mutex<Vec<(String, String)>>>,
    /// Copy image's PNGs read off-thread, for `poll` to put on the clipboard.
    copied: Arc<Mutex<Vec<Result<Vec<u8>, String>>>>,
}

impl State {
    /// Drops every decoded picture but those still being signed, whose replies are still taken.
    pub(super) fn forget(&mut self) {
        self.entries.retain(|_, e| e.request.is_some());
        self.menu = None;
    }
}

fn net_for<'a>(app: &'a mut App, env: Option<&str>) -> Option<&'a mut s5_net::Net> {
    match env {
        Some(env) => crate::env::net_of(app, env),
        None => app.src.net(),
    }
}

/// Starts loading the media views asked for, folds in signed URLs, and copies the images read.
/// Called from `attachments::poll`.
pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let waker = app.ui.panel.waker.clone();
    let wanted = std::mem::take(app.ui.chat.timeline.assets.media.wanted.get_mut());
    let mut changed = false;
    for (key, media, access) in wanted {
        if app.ui.chat.timeline.assets.media.entries.contains_key(&key) {
            continue;
        }
        let mut entry = Entry { media, request: None, loaded: None, failed: false };
        match access {
            Access::Direct(url) => entry.loaded = Some(Loaded::new(url, entry.media.name.clone(), Some(entry.media.kind), MARKDOWN_DECODE, waker.clone())),
            Access::Asset { resource, env, fallback } => match (net_for(app, env.as_deref()), fallback) {
                (Some(net), fallback) => entry.request = Some((net.request("assets.createUrl", json!({ "resource": resource })), env, fallback)),
                (None, Some(url)) => entry.loaded = Some(Loaded::new(url, entry.media.name.clone(), Some(entry.media.kind), MARKDOWN_DECODE, waker.clone())),
                (None, None) => entry.failed = true,
            },
        }
        app.ui.chat.timeline.assets.media.entries.insert(key, entry);
        changed = true;
    }
    let pending: Vec<(String, u64, Option<String>)> =
        app.ui.chat.timeline.assets.media.entries.iter().filter_map(|(k, e)| e.request.as_ref().map(|(id, env, _)| (k.clone(), *id, env.clone()))).collect();
    for (key, id, env) in pending {
        let Some(net) = net_for(app, env.as_deref()) else { continue };
        let Some(reply) = net.take_reply(id) else { continue };
        let base = net.base_url().trim_end_matches('/').to_owned();
        let Some(entry) = app.ui.chat.timeline.assets.media.entries.get_mut(&key) else { continue };
        let fallback = entry.request.take().and_then(|(_, _, fallback)| fallback);
        changed = true;
        let url = match reply {
            Ok(reply) => Some(format!("{base}{}", s(&reply, "relativeUrl"))),
            Err(_) => fallback,
        };
        match url {
            Some(url) => entry.loaded = Some(Loaded::new(url, entry.media.name.clone(), Some(entry.media.kind), MARKDOWN_DECODE, waker.clone())),
            None => entry.failed = true,
        }
    }
    let state = &mut app.ui.chat.timeline.assets.media;
    let errors = state.errors.lock().map(|mut e| std::mem::take(&mut *e)).unwrap_or_default();
    let copied = state.copied.lock().map(|mut c| std::mem::take(&mut *c)).unwrap_or_default();
    for (title, description) in errors {
        crate::shell::palette::toast_error(app, &title, Some(&description));
        changed = true;
    }
    for png in copied {
        match png.and_then(|png| rune::ClipboardImage::from_png(png).ok_or_else(|| "The image could not be converted to PNG.".to_owned())) {
            Ok(image) => {
                tx.copy_rich(rune::ClipboardContent::default().with_image(image));
                crate::shell::palette::toast(app, "Image copied", None);
            }
            Err(e) => crate::shell::palette::toast_error(app, "Could not copy image", Some(&e)),
        }
        changed = true;
    }
    changed
}

fn want(app: &App, key: &str, media: &Media, access: Access) {
    let state = &app.ui.chat.timeline.assets.media;
    if let Ok(mut wanted) = state.wanted.try_borrow_mut()
        && !wanted.iter().any(|(k, ..)| k == key)
    {
        wanted.push((key.to_owned(), media.clone(), access));
        // The view can't load; wake `poll` to.
        if let Some(wake) = &app.ui.panel.waker {
            wake();
        }
    }
}

/// What every Markdown view holding an image reads of the media state.
pub fn signature(app: &App, source: &str) -> String {
    if !source.contains("![") {
        return String::new();
    }
    let entries = &app.ui.chat.timeline.assets.media.entries;
    entries.iter().map(|(k, e)| format!("{k}={}{}", e.failed, e.loaded.as_ref().map(Loaded::stamp).unwrap_or_default())).collect::<Vec<_>>().join(";")
}

/// The media a key names, from Markdown or a sent attachment.
pub fn find(app: &App, key: &str) -> Option<(Media, Option<Loaded>)> {
    let assets = &app.ui.chat.timeline.assets;
    if let Some(e) = assets.media.entries.get(key) {
        return Some((e.media.clone(), e.loaded.clone()));
    }
    let loaded = assets.ready.get(key)?;
    let media = Media { kind: loaded.kind?, name: loaded.name.clone(), reference: None, open_file: None };
    Some((media, Some(loaded.clone())))
}

/// Retry video: forget the source so it signs and loads again.
fn retry(app: &mut App, key: &str) {
    let assets = &mut app.ui.chat.timeline.assets;
    assets.media.entries.remove(key);
    assets.ready.remove(key);
    assets.failed.remove(key);
}

/// The `![alt](src)` images of a paragraph holding nothing else.
pub fn images(block: &str) -> Option<Vec<(String, String)>> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut out = Vec::new();
    let mut open: Option<(String, String)> = None;
    let mut paragraphs = 0;
    for event in Parser::new(block) {
        match event {
            Event::Start(Tag::Paragraph) => {
                paragraphs += 1;
                if paragraphs > 1 {
                    return None;
                }
            }
            Event::End(TagEnd::Paragraph) | Event::SoftBreak | Event::HardBreak => {}
            Event::Start(Tag::Image { dest_url, .. }) if open.is_none() => open = Some((String::new(), dest_url.to_string())),
            Event::End(TagEnd::Image) => out.push(open.take()?),
            Event::Text(piece) | Event::Code(piece) if open.is_some() => open.as_mut()?.0.push_str(&piece),
            Event::Text(piece) if piece.trim().is_empty() => {}
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

/// ChatMarkdown's `img`: the entry key, the media, and how to reach it. Audio and host files
/// that are not media stay with the Markdown text.
fn resolve(alt: &str, src: &str, root: Option<&str>, thread: Option<&str>) -> Option<(String, Media, Access)> {
    let source = classify(src, root)?;
    let found = kind(match &source {
        Source::Direct(u) | Source::File(u) => u,
    });
    let kind = match (found, &source) {
        (Some("image"), _) | (None, Source::Direct(_)) => Kind::Image,
        (Some("video"), _) => Kind::Video,
        _ => return None,
    };
    Some(match source {
        Source::Direct(uri) => {
            let url = if uri.starts_with("//") { format!("https:{uri}") } else { uri.clone() };
            (format!("media:{uri}"), Media { kind, name: alt.to_owned(), reference: url_reference(&uri), open_file: None }, Access::Direct(url))
        }
        Source::File(path) => {
            let thread = thread?;
            let reference = file_reference(&path, root);
            let open_file = match &reference {
                Reference::File { relative, .. } => relative.clone(),
                Reference::Url(_) => None,
            };
            let resource = json!({"_tag": "media-file", "threadId": thread, "path": path});
            (format!("media:{thread}:{path}"), Media { kind, name: alt.to_owned(), reference: Some(reference), open_file }, Access::Asset { resource, env: None, fallback: None })
        }
    })
}

/// A Markdown paragraph of nothing but images or videos, drawn as them (standalone media).
pub fn block(app: &App, block: &str, t: &T) -> Option<El> {
    if !block.contains("![") {
        return None;
    }
    let images = images(block)?;
    let root = super::workspace_root(app);
    let thread = match app.frame_route() {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let resolved = images.iter().map(|(alt, src)| resolve(alt, src, root.as_deref(), thread.as_deref())).collect::<Option<Vec<_>>>()?;
    let gallery: Vec<String> = resolved.iter().filter(|(_, m, _)| m.kind == Kind::Image).map(|(k, ..)| k.clone()).collect();
    let mut out = col().w_full().gap(8.0);
    for (key, media, access) in resolved {
        let entry = app.ui.chat.timeline.assets.media.entries.get(&key);
        let framed = matches!(access, Access::Asset { .. });
        if entry.is_none() {
            want(app, &key, &media, access);
        }
        let loaded = entry.and_then(|e| e.loaded.as_ref());
        let failed = entry.is_some_and(|e| e.failed);
        let shown = match media.kind {
            Kind::Image => picture(&key, &media, loaded, failed, framed, &gallery, t),
            Kind::Video => {
                let original = match &media.reference {
                    Some(Reference::Url(url)) => Some(url.as_str()),
                    _ => None,
                };
                col().w_full().max_w(MAX_SIDE).child(video(&key, &media.name, loaded, failed, original, t))
            }
            // `resolve` leaves audio with the Markdown text.
            Kind::Audio => continue,
        };
        out = out.child(with_actions(shown, &key, &media, true));
    }
    Some(out)
}

/// ChatMarkdownImage in prose, which web draws inline-block: a paragraph of words and images as
/// one text, once each image has loaded. Until then, or when one can't load, rune_kit's
/// Markdown draws it.
pub fn inline(app: &App, block: &str, font: rune::tok::TextStyle, ink: Color, t: &T) -> Option<El> {
    if !block.contains("![") {
        return None;
    }
    let root = super::workspace_root(app);
    let thread = match app.frame_route() {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let image = |alt: &str, src: &str| {
        let (key, media, access) = resolve(alt, src, root.as_deref(), thread.as_deref())?;
        if media.kind != Kind::Image {
            return None;
        }
        let framed = matches!(access, Access::Asset { .. });
        let Some(entry) = app.ui.chat.timeline.assets.media.entries.get(&key) else {
            want(app, &key, &media, access);
            return None;
        };
        let loaded = entry.loaded.as_ref().filter(|l| !entry.failed && !l.failed())?;
        let (w, h) = loaded.pixels.size();
        if w == 0 || h == 0 {
            return None;
        }
        let ratio = w as f32 / h as f32;
        let width = (w as f32).min(MAX_SIDE).min(MAX_SIDE * ratio);
        let label = alt.replace('\\', "\\\\").replace('[', "\\[").replace(']', "\\]");
        // An inline-block image's baseline is its bottom edge.
        let height = width / ratio;
        Some(super::inline::Picture { pixels: loaded.pixels.clone(), size: [width, height], copy: format!("![{label}]({src})"), open: Some(key), baseline: Some(height), framed })
    };
    super::inline::paragraph(block, Vec::new(), &image, font, ink, t)
}

/// `ChatMarkdownImage` standalone: the picture at its size within 30rem, a muted frame while it
/// loads, and "Image unavailable" when it can't.
fn picture(key: &str, media: &Media, loaded: Option<&Loaded>, failed: bool, framed: bool, gallery: &[String], t: &T) -> El {
    let frame = |e: El| e.radius(8.0).border(1.0, t.c.border.alpha_mul(0.4));
    let size = loaded.map(|l| l.pixels.size()).filter(|(w, h)| *w > 0 && *h > 0);
    if failed || loaded.is_some_and(Loaded::failed) {
        return frame(col().w_full().max_w(MAX_SIDE).aspect_ratio(16.0 / 9.0).center().padding(8.0).fill(t.c.muted.alpha_mul(0.6)))
            .role("alert")
            .child(unavailable_label("Image unavailable", &media.name, theme::TEXT_XS, t));
    }
    let (Some(loaded), Some((w, h))) = (loaded, size) else {
        return frame(el().w_full().max_w(MAX_SIDE).aspect_ratio(16.0 / 9.0).fill(t.c.muted.alpha_mul(0.6))).role("status").label("Loading image");
    };
    let ratio = w as f32 / h as f32;
    let shown = el().w_full().max_w((w as f32).min(MAX_SIDE).min(MAX_SIDE * ratio)).aspect_ratio(ratio).pixels_fit(&loaded.pixels, SurfaceFit::Fit);
    let shown = if framed { frame(shown).clip_rounded(8.0) } else { shown };
    let (open, gallery) = (key.to_owned(), gallery.to_vec());
    shown
        .focusable(true)
        .role("button")
        .cursor(rune::Cursor::Pointer)
        .label(format!("Preview {}", media.display_name()))
        .on_press(move |app: &mut App| {
            let assets = &mut app.ui.chat.timeline.assets;
            assets.open = Some(open.clone());
            assets.gallery = gallery.clone();
        })
}

/// `ChatMarkdownMediaUnavailableLabel`.
fn unavailable_label(what: &str, alt: &str, font: rune::TextStyle, t: &T) -> El {
    let label = [what, alt.trim()].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ");
    row()
        .gap(6.0)
        .center_y()
        .child(icon(&icons::TRIANGLE_ALERT, 14.0).color(t.c.muted_foreground))
        .child(text(label).font(font).color(t.c.muted_foreground).shrink().min_w(0.0))
}

/// MediaVideoPlayer: the video, pressed to play or pause. One that can't play (Rune plays video
/// only on macOS) shows web's failure card: Retry video, and Open in browser for a web URL.
pub fn video(key: &str, label: &str, loaded: Option<&Loaded>, failed: bool, original: Option<&str>, t: &T) -> El {
    if failed || loaded.is_some_and(Loaded::failed) {
        let src = loaded.map(|l| l.url.as_str());
        let retry_key = key.to_owned();
        let mut buttons = row().gap(8.0).center().child(
            kit::Button::new("Retry video")
                .variant("secondary")
                .size("sm")
                .icon(&icons::ROTATE_CW)
                .on_press(move |app: &mut App| retry(app, &retry_key))
                .build(t),
        );
        if let Some((open, url)) = open_link(original, src) {
            buttons = buttons.child(kit::Button::new(open).variant("secondary").size("sm").icon(&icons::EXTERNAL_LINK).build(t).on_press_tx(move |_: &mut App, tx| {
                let _ = tx.open_url(&url);
            }));
        }
        return col()
            .w_full()
            .aspect_ratio(16.0 / 9.0)
            .min_h(112.0)
            .center()
            .gap(12.0)
            .padding(16.0)
            .radius(8.0)
            .border(1.0, t.c.border.alpha_mul(0.4))
            .fill(t.c.muted.alpha_mul(0.4))
            .role("alert")
            .child(unavailable_label("Video unavailable", label, theme::TEXT_SM, t))
            .child(buttons);
    }
    let Some(player) = loaded.and_then(|l| l.player.clone()) else {
        return el().w_full().aspect_ratio(16.0 / 9.0).radius(8.0).fill(t.c.muted.alpha_mul(0.6)).role("status").label("Loading video");
    };
    let (w, h) = player.natural_size().unwrap_or((16, 9));
    let playing = player.is_playing();
    let name = if label.trim().is_empty() { "Play video".to_owned() } else { format!("Play {label}") };
    let toggle = player.clone();
    let mut shown = el()
        .w_full()
        .aspect_ratio(w as f32 / h.max(1) as f32)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(Color::BLACK)
        .video(&player, SurfaceFit::Fit)
        .center()
        .focusable(true)
        .role("button")
        .cursor(rune::Cursor::Pointer)
        .label(if playing { "Pause video".to_owned() } else { name })
        .on_press(move |_: &mut App| if toggle.is_playing() { toggle.pause() } else { toggle.play() });
    if !playing {
        shown = shown.child(el().size(32.0, 32.0).center().radius(theme::radius::FULL).fill(Color::BLACK.alpha(0.5)).child(icon(&icons::PLAY, 16.0).color(Color::WHITE)));
    }
    shown
}

/// Where a pull request's media is read when its host keeps it behind a credential: `fetch`
/// through `env`'s server, with the credential it holds for the checkout at `cwd`.
pub struct Host<'a> {
    pub env: &'a str,
    pub cwd: &'a str,
    pub fetch: &'a str,
}

/// The entry `linked` files a URL under, for the lightbox's gallery.
pub fn linked_key(url: &str, host: Option<&Host>) -> String {
    match host {
        Some(h) => format!("media:{}:{}:{}", h.env, h.cwd, h.fetch),
        None => format!("media:{url}"),
    }
}

/// A pull request's linked picture or video (PullRequestMarkdown): a picture opens in the
/// lightbox among `gallery`, the body's other pictures.
pub fn linked(app: &App, url: &str, name: &str, kind: Kind, host: Option<&Host>, gallery: &[String], t: &T) -> El {
    let name = if kind == Kind::Video && name.trim().is_empty() { "Pull request video" } else { name };
    let media = Media { kind, name: name.to_owned(), reference: url_reference(url), open_file: None };
    let key = linked_key(url, host);
    let access = match host {
        Some(h) => Access::Asset { resource: json!({"_tag": "github-media", "cwd": h.cwd, "url": h.fetch}), env: Some(h.env.to_owned()), fallback: Some(h.fetch.to_owned()) },
        None => Access::Direct(url.to_owned()),
    };
    let entry = app.ui.chat.timeline.assets.media.entries.get(&key);
    if entry.is_none() {
        want(app, &key, &media, access);
    }
    let loaded = entry.and_then(|e| e.loaded.as_ref());
    let failed = entry.is_some_and(|e| e.failed);
    let shown = match kind {
        // Pull requests only link pictures and videos; the player also plays sound.
        Kind::Video | Kind::Audio => col().w_full().max_w(MAX_SIDE).child(video(&key, name, loaded, failed, Some(url), t)),
        Kind::Image => picture(&key, &media, loaded, failed, false, gallery, t),
    };
    with_actions(shown, &key, &media, true)
}

/// MediaActions' trigger: the source as a tooltip, and right-click for the actions.
pub fn with_actions(content: El, key: &str, media: &Media, tooltip: bool) -> El {
    let help = match &media.reference {
        Some(Reference::File { path, .. }) => path.clone(),
        Some(Reference::Url(url)) => url.clone(),
        None => media.display_name(),
    };
    let key = key.to_owned();
    let content = content.on_secondary_press(move |app: &mut App, at: PointerPress, _: &mut Tx| app.ui.chat.timeline.assets.media.menu = Some((key.clone(), at.x, at.y)));
    if tooltip { content.help(help) } else { content }
}

/// The open media's actions menu.
pub fn menu(app: &App, k: &rune_kit::Kit, layers: &mut Vec<El>) {
    let Some((key, x, y)) = app.ui.chat.timeline.assets.media.menu.clone() else { return };
    let Some((media, loaded)) = find(app, &key) else { return };
    let available = loaded.is_some();
    let close = |app: &mut App| app.ui.chat.timeline.assets.media.menu = None;
    let mut menu = kit::Menu::new("media-actions-menu").native(true).width(198.0).open(true).on_close(close).at(x, y);
    for (action, label, disabled) in actions(&media, available) {
        let key = key.clone();
        menu = menu.item(kit::MenuItem::new(label).disabled(disabled).on_select_tx(move |app: &mut App, tx: &mut Tx| {
            close(app);
            run(app, &key, action, tx);
        }));
    }
    menu.overlay(k, layers);
}

fn run(app: &mut App, key: &str, action: Action, tx: &mut Tx) {
    let Some((media, loaded)) = find(app, key) else { return };
    match (action, &media.reference) {
        (Action::CopyFullPath | Action::CopyRelativePath, Some(Reference::File { path, relative })) => {
            let text = if action == Action::CopyFullPath { path.clone() } else { relative.clone().unwrap_or_default() };
            tx.copy(clipboard_path(&text, path));
            crate::shell::palette::toast(app, "Path copied", None);
        }
        (Action::CopyUrl, Some(Reference::Url(url))) => {
            tx.copy(url.clone());
            crate::shell::palette::toast(app, "URL copied", None);
        }
        (Action::OpenFile, _) => {
            if let Some(path) = media.open_file.clone() {
                app.ui.chat.timeline.assets.open = None;
                crate::panel::open_file(app, path);
            }
        }
        (Action::Save, _) => save(app, &media, loaded, tx),
        (Action::CopyImage, _) => {
            let Some(url) = loaded.map(|l| l.url) else { return };
            let (copied, waker) = (app.ui.chat.timeline.assets.media.copied.clone(), app.ui.panel.waker.clone());
            std::thread::spawn(move || {
                let png = read_png(&url);
                if let Ok(mut list) = copied.lock() {
                    list.push(png);
                }
                if let Some(wake) = &waker {
                    wake();
                }
            });
        }
        _ => {}
    }
}

/// Save image / Save video: a save dialog, then a download of the source.
fn save(app: &mut App, media: &Media, loaded: Option<Loaded>, tx: &mut Tx) {
    let Some(loaded) = loaded else { return };
    let noun = media.kind.noun();
    let name = media.reference.as_ref().and_then(file_name).unwrap_or_else(|| media.display_name());
    let options = rune::FileDialogOptions { title: format!("Save {noun}"), file_name: Some(name), ..Default::default() };
    let errors = app.ui.chat.timeline.assets.media.errors.clone();
    let waker = app.ui.panel.waker.clone();
    tx.save_file(options, move |app: &mut App, picked: rune::FileDialogResult, _| {
        let Ok(Some(path)) = picked else { return };
        let title = format!("Could not save {noun}");
        crate::shell::palette::toast(app, "Download started", None);
        let (url, errors, waker) = (loaded.url.clone(), errors.clone(), waker.clone());
        std::thread::spawn(move || {
            if let Err(e) = download(&url, &path) {
                if let Ok(mut list) = errors.lock() {
                    list.push((title, e));
                }
                if let Some(wake) = &waker {
                    wake();
                }
            }
        });
    });
}

/// `downloadFileAttachment`: a save dialog named for the file, then its bytes from `url`.
pub fn save_attachment(app: &mut App, url: String, name: String, tx: &mut Tx) {
    let options = rune::FileDialogOptions { title: format!("Download {name}"), file_name: Some(name.clone()), ..Default::default() };
    let errors = app.ui.chat.timeline.assets.media.errors.clone();
    let waker = app.ui.panel.waker.clone();
    tx.save_file(options, move |app: &mut App, picked: rune::FileDialogResult, _| {
        let Ok(Some(path)) = picked else { return };
        crate::shell::palette::toast(app, "Download started", None);
        let (url, errors, waker, title) = (url.clone(), errors.clone(), waker.clone(), format!("Could not download {name}"));
        std::thread::spawn(move || {
            let saved = request(&url).and_then(|response| {
                let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                std::io::copy(&mut response.into_reader(), &mut file).map(drop).map_err(|e| e.to_string())
            });
            if let Err(e) = saved {
                if let Ok(mut list) = errors.lock() {
                    list.push((title, e));
                }
                if let Some(wake) = &waker {
                    wake();
                }
            }
        });
    });
}

/// Scenes: a Markdown media entry in a given state, drawn without loading anything.
pub(super) fn scene_entry(app: &mut App, alt: &str, src: &str, failed: bool) -> Option<String> {
    let root = super::workspace_root(app);
    let thread = match app.frame_route() {
        Route::Thread(id) => Some(id.clone()),
        _ => None,
    };
    let (key, media, _) = resolve(alt, src, root.as_deref(), thread.as_deref())?;
    let loaded = (!failed).then(|| checker(&media));
    app.ui.chat.timeline.assets.media.entries.insert(key.clone(), Entry { media, request: None, loaded, failed });
    Some(key)
}

/// Scenes: a pull request's linked media under `key` (`linked_key`), drawn without loading.
pub fn scene_linked(app: &mut App, key: &str, name: &str, kind: Kind, failed: bool) {
    let media = Media { kind, name: name.to_owned(), reference: None, open_file: None };
    let loaded = (!failed).then(|| checker(&media));
    app.ui.chat.timeline.assets.media.entries.insert(key.to_owned(), Entry { media, request: None, loaded, failed });
}

/// Scenes: the lightbox open on the first of `gallery`.
pub fn scene_open(app: &mut App, gallery: Vec<String>) {
    let assets = &mut app.ui.chat.timeline.assets;
    assets.open = gallery.first().cloned();
    assets.gallery = gallery;
}

fn checker(media: &Media) -> Loaded {
    let loaded = Loaded::new(String::new(), media.name.clone(), None, MARKDOWN_DECODE, None);
    let (w, h) = (640u32, 360u32);
    let rgba = (0..w * h).flat_map(|i| if (i % w / 40 + i / w / 40) % 2 == 0 { [96, 125, 180, 255] } else { [140, 170, 220, 255] }).collect();
    loaded.pixels.set(w, h, rgba);
    Loaded { kind: Some(media.kind), ..loaded }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_media_signs_on_its_environment_and_falls_back_to_the_host_url() {
        let mut app = App::new();
        app.src = crate::data::Source::Fixture(crate::data::empty_state());
        app.ui.env.active = crate::env::LOCAL.into();
        // An offline client refuses every request at once: its worker is stopped.
        crate::env::park(&mut app, "far", crate::data::Source::Live(s5_net::Net::offline()));
        let host = Host { env: "far", cwd: "/srv/app", fetch: "https://example.invalid/user-attachments/assets/ab" };
        let key = linked_key("https://example.invalid/user-attachments/assets/ab?x=1", Some(&host));
        let media = Media { kind: Kind::Image, name: String::new(), reference: None, open_file: None };
        let asset = |env: &str, fallback: Option<&str>| Access::Asset { resource: json!({}), env: Some(env.into()), fallback: fallback.map(str::to_owned) };
        want(&app, &key, &media, asset("far", Some(host.fetch)));
        want(&app, "media:gone", &media, asset("gone", None));
        want(&app, "media:gone-fallback", &media, asset("gone", Some("https://example.invalid/b")));
        poll(&mut app, &mut Tx::default());
        let entries = &app.ui.chat.timeline.assets.media.entries;
        let url = |k: &str| entries[k].loaded.as_ref().map(|l| l.url.clone());
        assert_eq!(url(key.as_str()).as_deref(), Some(host.fetch), "FAILED: far refused, so the picture reads the host's URL");
        assert!(entries["media:gone"].failed && url("media:gone").is_none(), "FAILED: not connected, nothing to fall back to");
        assert_eq!(url("media:gone-fallback").as_deref(), Some("https://example.invalid/b"));
        assert!(entries.values().all(|e| e.request.is_none()));
    }

    #[test]
    fn media_kinds_follow_the_extension() {
        assert_eq!(kind("shots/a.PNG"), Some("image"));
        assert_eq!(kind("https://x.dev/v/demo.mp4?t=3#x"), Some("video"));
        assert_eq!(kind("<clip.ogg>"), Some("audio"));
        assert_eq!(kind("data:image/png;base64,AAAA"), Some("image"));
        assert_eq!(kind("https://x.dev/image"), None);
        assert_eq!(kind("notes.md"), None);
        assert_eq!(attachment_kind("video/quicktime", "a.mov"), Some(Kind::Video));
        assert_eq!(attachment_kind("", "a.webm"), Some(Kind::Video));
        assert_eq!(attachment_kind("application/pdf", "a.mp4"), None);
        assert_eq!(attachment_kind("audio/mpeg", "memo.mp3"), Some(Kind::Audio));
        assert_eq!(attachment_kind("", "memo.m4a"), Some(Kind::Audio));
        assert_eq!(attachment_kind("image/png", "a.png"), Some(Kind::Image));
    }

    #[test]
    fn sources_classify_like_markdown_images() {
        let root = Some("/home/me/app");
        assert_eq!(classify("https://x.dev/a.png", root), Some(Source::Direct("https://x.dev/a.png".into())));
        assert_eq!(classify("//cdn.dev/a.png", None), Some(Source::Direct("//cdn.dev/a.png".into())));
        assert_eq!(classify("docs/a%20b.png", root), Some(Source::File("/home/me/app/docs/a b.png".into())));
        assert_eq!(classify("file:///tmp/a.png", None), Some(Source::File("/tmp/a.png".into())));
        assert_eq!(classify("file:///C:/x/a.png", None), Some(Source::File("C:/x/a.png".into())));
        assert_eq!(classify("C:\\repo\\a.png", None), Some(Source::File("C:\\repo\\a.png".into())));
        for blocked in ["docs/a.png", "#top", "blob:https://x/1", "mailto:a@b.co", "~/a.png"] {
            assert_eq!(classify(blocked, None), None, "{blocked}");
        }
        assert_eq!(classify("relative.png", Some("C:\\repo")), Some(Source::File("C:\\repo\\relative.png".into())));
    }

    #[test]
    fn references_name_the_file_and_its_workspace_path() {
        let inside = file_reference("/home/me/app/docs/./shot.png", Some("/home/me/app/"));
        assert_eq!(inside, Reference::File { path: "/home/me/app/docs/./shot.png".into(), relative: Some("docs/shot.png".into()) });
        assert_eq!(file_reference("/etc/a.png", Some("/home/me/app")), Reference::File { path: "/etc/a.png".into(), relative: None });
        let windows = file_reference("c:\\Repo\\img\\a.png", Some("C:/repo"));
        assert_eq!(windows, Reference::File { path: "c:\\Repo\\img\\a.png".into(), relative: Some("img/a.png".into()) });
        assert_eq!(file_name(&windows).as_deref(), Some("a.png"));
        assert_eq!(file_name(&Reference::Url("https://x.dev/v/my%20clip.mp4?x=1".into())).as_deref(), Some("my clip.mp4"));
        assert_eq!(url_reference("data:image/png;base64,AA"), None);
        assert_eq!(clipboard_path("img/a.png", "C:\\repo\\img\\a.png"), "img\\a.png");
        assert_eq!(clipboard_path("img/a.png", "/repo/img/a.png"), "img/a.png");
    }

    #[test]
    fn actions_match_web_media_menus() {
        let labels = |media: &Media, available: bool| actions(media, available).into_iter().map(|(_, l, d)| format!("{l}{}", if d { " (off)" } else { "" })).collect::<Vec<_>>();
        let file = Media {
            kind: Kind::Image,
            name: "Shot".into(),
            reference: Some(file_reference("/repo/docs/a.png", Some("/repo"))),
            open_file: Some("docs/a.png".into()),
        };
        assert_eq!(labels(&file, true), ["Copy full path", "Copy relative path", "Open in file viewer", "Save image", "Copy image"]);
        let web = Media { kind: Kind::Video, name: String::new(), reference: url_reference("https://x.dev/a.mp4"), open_file: None };
        assert_eq!(labels(&web, false), ["Copy URL", "Save video (off)"]);
        assert_eq!(open_link(Some("https://x.dev/a.mp4"), Some("http://host/asset")), Some(("Open original", "https://x.dev/a.mp4".into())));
        assert_eq!(open_link(None, Some("http://host/asset")), Some(("Open in browser", "http://host/asset".into())));
        assert_eq!(open_link(None, Some("data:video/mp4;base64,AA")), None);
    }

    #[test]
    fn pictures_decode_small_and_copy_reads_the_source_again() {
        use base64::Engine;
        let encoded = |format: image::ImageFormat| {
            let mut out = std::io::Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image::RgbaImage::new(1200, 300)).write_to(&mut out, format).unwrap();
            out.into_inner()
        };
        let (png, bmp) = (encoded(image::ImageFormat::Png), encoded(image::ImageFormat::Bmp));
        let thumb = decode(&png, 480).unwrap();
        assert_eq!((thumb.width(), thumb.height()), (480, 120));
        assert_eq!(decode(&png, MARKDOWN_DECODE).unwrap().width(), 960);
        let data_url = |mime: &str, bytes: &[u8]| format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes));
        assert_eq!(read_png(&data_url("image/png", &png)).unwrap(), png);
        let converted = image::load_from_memory_with_format(&read_png(&data_url("image/bmp", &bmp)).unwrap(), image::ImageFormat::Png).unwrap();
        assert_eq!((converted.width(), converted.height()), (1200, 300));
        assert_eq!(read_png("data:image/png;base64,!!").unwrap_err(), "This inline media could not be decoded.");
    }

    #[test]
    fn only_image_paragraphs_become_media() {
        assert_eq!(images("![a](x.png) ![b](y.mp4)"), Some(vec![("a".into(), "x.png".into()), ("b".into(), "y.mp4".into())]));
        assert_eq!(images("See ![a](x.png)"), None);
        assert_eq!(images("[![a](x.png)](https://x.dev)"), None);
        let (key, media, _) = resolve("Demo", "clips/demo.mp4", Some("/repo"), Some("t1")).unwrap();
        assert_eq!(key, "media:t1:/repo/clips/demo.mp4");
        assert_eq!((media.kind, media.open_file.as_deref()), (Kind::Video, Some("clips/demo.mp4")));
        assert_eq!(resolve("", "https://x.dev/badge", None, None).map(|(_, m, _)| m.kind), Some(Kind::Image));
        assert!(resolve("", "voice.mp3", Some("/repo"), Some("t1")).is_none());
        assert!(resolve("", "src/main.rs", Some("/repo"), Some("t1")).is_none());
    }
}
