mod archive;
mod large_text;
mod notebook;
mod office;
mod scenes;
mod sqlite;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{El, Tx, col, el, row, text};
pub(super) use scenes::SCENES;
use serde_json::json;

use self::{archive::Archive, large_text::LargeText, sqlite::Database};
use super::browser::{Cmd, Live};
use super::file_preview::{self as fp, Kind};
use crate::theme::{self, AlphaMul, T};
use crate::{App, kit};

pub(super) const BINARY_LIMIT: usize = 50 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Large(Option<char>),
    Archive,
    Sqlite,
    Pdf,
    Html,
    Audio,
    Video,
    Font,
    Notebook,
    Office(office::Format),
}

impl Mode {
    pub(super) fn for_file(kind: Kind, path: &str, rendered: bool) -> Option<Self> {
        Some(match kind {
            Kind::Pdf => Mode::Pdf,
            Kind::Html if rendered => Mode::Html,
            Kind::Video => Mode::Video,
            Kind::Audio => Mode::Audio,
            Kind::Font => Mode::Font,
            Kind::Notebook => Mode::Notebook,
            Kind::Office => Mode::Office(office::Format::from_path(path)?),
            Kind::Archive if [".sqlite", ".sqlite3", ".db"].iter().any(|ext| path.to_ascii_lowercase().ends_with(ext)) => Mode::Sqlite,
            Kind::Archive => Mode::Archive,
            _ => return None,
        })
    }

    fn limit(self) -> (usize, &'static str) {
        match self {
            Mode::Large(_) => (large_text::BYTE_LIMIT, "Text previews support files up to 256 MB."),
            Mode::Notebook => (8 * 1024 * 1024, "Notebook preview limited to files under 8 MB."),
            Mode::Font => (10 * 1024 * 1024, "Font previews are limited to 10 MB."),
            _ => (BINARY_LIMIT, "Preview limited to files under 50 MB."),
        }
    }
}

enum Content {
    Large(LargeText),
    Archive(Archive),
    Sqlite(Database),
    Browser(Frame),
    Notebook(notebook::NotebookView),
    Office(office::Office),
}

pub(super) struct Viewer {
    mode: Mode,
    pending: Option<u64>,
    job: Option<Job<Content>>,
    content: Option<Result<Content, String>>,
}

impl Viewer {
    pub(super) fn start(app: &mut App, mode: Mode, cwd: &str, path: &str) -> Option<Self> {
        let absolute = if path.starts_with('/') { path.to_owned() } else { format!("{}/{path}", cwd.trim_end_matches('/')) };
        let resource = match super::context(app) {
            Some((thread, _)) if matches!(mode, Mode::Html) => json!({"_tag": "workspace-file", "threadId": thread, "path": absolute}),
            Some((thread, _)) if matches!(mode, Mode::Pdf | Mode::Audio | Mode::Video) => json!({"_tag": "media-file", "threadId": thread, "path": absolute}),
            Some((thread, _)) => json!({"_tag": "media-file", "threadId": thread, "path": absolute, "disposition": "attachment"}),
            None => json!({"_tag": "draft-workspace-file", "cwd": cwd, "path": absolute}),
        };
        let id = app.src.net()?.request("assets.createUrl", json!({"resource": resource}));
        Some(Self { mode, pending: Some(id), job: None, content: None })
    }

    fn ready(mode: Mode, content: Result<Content, String>) -> Self {
        Self { mode, pending: None, job: None, content: Some(content) }
    }

    pub(super) fn view(&self, app: &App, rendered: bool, wrap: bool, t: &T) -> El {
        match self.content.as_ref() {
            None => loading(t),
            Some(Err(error)) => failure(error, t),
            Some(Ok(Content::Large(file))) => file.view(app, rendered, wrap, t),
            Some(Ok(Content::Archive(archive))) => archive.view(t),
            Some(Ok(Content::Sqlite(database))) => database.view(t),
            Some(Ok(Content::Browser(frame))) => frame.view(app, t),
            Some(Ok(Content::Notebook(notebook))) => notebook.view(app, t),
            Some(Ok(Content::Office(office))) => office.view(t),
        }
    }

    fn settle(&mut self) -> bool {
        let Some(result) = self.job.as_ref().and_then(Job::take) else { return false };
        self.job = None;
        self.content = Some(result);
        true
    }

    fn poll_content(&mut self) -> bool {
        match self.content.as_mut().and_then(|content| content.as_mut().ok()) {
            Some(Content::Sqlite(database)) => database.poll(),
            Some(Content::Archive(archive)) => archive.poll(),
            Some(Content::Browser(frame)) => frame.poll(),
            Some(Content::Notebook(notebook)) => notebook.poll(),
            _ => false,
        }
    }

    pub(super) fn loading_full_text(&self) -> bool {
        matches!(self.mode, Mode::Large(_)) && self.content.is_none()
    }
}

fn on_attachment(app: &App) -> bool {
    matches!(super::active_target(app), Some(super::tabs::Target::Attachment(_)))
}

/// The viewer on screen: the active attachment tab's, otherwise the open file's.
fn active(app: &App) -> Option<&Content> {
    let viewer = if on_attachment(app) { app.ui.panel.attachment.as_ref().map(|(_, viewer)| viewer) } else { app.ui.panel.files.viewer.as_ref() };
    viewer?.content.as_ref()?.as_ref().ok()
}

fn active_mut(app: &mut App) -> Option<&mut Content> {
    let viewer = if on_attachment(app) { app.ui.panel.attachment.as_mut().map(|(_, viewer)| viewer) } else { app.ui.panel.files.viewer.as_mut() };
    viewer?.content.as_mut()?.as_mut().ok()
}

/// The shown file's name, for labels inside a viewer.
fn shown_name(app: &App) -> &str {
    match super::active_target(app) {
        Some(super::tabs::Target::Attachment(id)) => crate::chat::timeline::attachments::panel_name(app, id).unwrap_or_default(),
        _ => app.ui.panel.files.selected.as_str(),
    }
}

/// The line a search or link marked, which only workspace files have.
fn shown_line(app: &App) -> Option<usize> {
    if on_attachment(app) { None } else { app.ui.panel.files.line }
}

pub(super) fn full_text(app: &App) -> Option<&str> {
    match active(app) {
        Some(Content::Large(file)) => Some(file.contents()),
        _ => None,
    }
}

/// Scrolls the open file to its marked line, in the full-text viewer when one is showing.
pub(super) fn reveal(app: &App, tx: &mut Tx) {
    match active(app) {
        Some(Content::Large(file)) => file.reveal(app.ui.panel.files.line, tx),
        _ => tx.scroll_center("file-line"),
    }
}

pub(super) fn overlays(app: &App, t: &T, layers: &mut Vec<El>) {
    if let Some(Content::Sqlite(database)) = active(app) {
        database.overlays(t, layers);
    }
}

pub(super) fn close(app: &mut App) {
    hide_frame(app, false);
    app.ui.panel.files.viewer = None;
}

pub(super) fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = sync_attachment(app);
    if !super::files_showing(app) {
        changed |= hide_frame(app, false);
    }
    if !on_attachment(app) {
        changed |= hide_frame(app, true);
    }
    if let Some((_, viewer)) = app.ui.panel.attachment.as_mut() {
        changed |= viewer.settle() | viewer.poll_content();
    }
    let Some(viewer) = app.ui.panel.files.viewer.as_mut() else { return changed };
    if let Some(id) = viewer.pending
        && let Some(reply) = app.src.net().and_then(|net| net.take_reply(id))
    {
        viewer.pending = None;
        match reply {
            Ok(v) => {
                let revision = v["fileRevision"].as_str();
                let mut url = super::files::asset_url(&app.src, &v);
                if let (Ok(mut address), Some(revision)) = (s5_net::url::Url::parse(&url), revision) {
                    address.query_pairs_mut().append_pair("file-revision", revision);
                    url = address.to_string();
                }
                let name = app.ui.panel.files.selected.clone();
                let mode = viewer.mode;
                let wake = app.ui.panel.waker.clone();
                viewer.job = Some(Job::spawn(wake, move || open(mode, &url, &name)));
            }
            Err(_) => viewer.content = Some(Err("Unable to load file preview.".into())),
        }
        changed = true;
    }
    if viewer.settle() {
        if let Some(Ok(Content::Large(file))) = &viewer.content {
            file.reveal(app.ui.panel.files.line, tx);
        }
        changed = true;
    }
    changed | viewer.poll_content()
}

/// Closes the live browser behind a viewer that is no longer on screen.
fn hide_frame(app: &mut App, attachment: bool) -> bool {
    let viewer = if attachment { app.ui.panel.attachment.as_mut().map(|(_, viewer)| viewer) } else { app.ui.panel.files.viewer.as_mut() };
    let Some(Ok(Content::Browser(frame))) = viewer.and_then(|viewer| viewer.content.as_mut()) else { return false };
    let Some(live) = frame.live.take() else { return false };
    frame.size = [0; 2];
    super::preview::inline_frame_cmd(app, Cmd::LiveClose(live.id));
    true
}

/// The format-aware mode for a chat attachment Rune can't show as a picture or player.
fn attachment_mode(name: &str) -> Option<Mode> {
    let kind = fp::kind(name);
    Mode::for_file(kind, name, true).or(match kind {
        Kind::Table(delimiter) => Some(Mode::Large(Some(delimiter))),
        Kind::Text | Kind::Markdown | Kind::Mermaid => Some(Mode::Large(None)),
        _ => None,
    })
}

/// Starts the viewer for the active attachment tab once its URL is served.
fn sync_attachment(app: &mut App) -> bool {
    let Some(super::tabs::Target::Attachment(id)) = super::active_target(app) else { return false };
    if app.ui.panel.attachment.as_ref().is_some_and(|(open, _)| open == id) {
        return false;
    }
    let Some((url, name)) = crate::chat::timeline::attachments::panel_file(app, id) else { return false };
    let Some(mode) = attachment_mode(name) else { return false };
    let (id, url, name) = (id.clone(), url.to_owned(), name.to_owned());
    close_attachment(app);
    let job = Job::spawn(app.ui.panel.waker.clone(), move || open(mode, &url, &name));
    app.ui.panel.attachment = Some((id, Viewer { mode, pending: None, job: Some(job), content: None }));
    true
}

pub(super) fn close_attachment(app: &mut App) {
    hide_frame(app, true);
    app.ui.panel.attachment = None;
}

/// The active attachment tab's viewer, when its format has one.
pub(super) fn attachment_view(app: &App, id: &str, t: &T) -> Option<El> {
    let (_, viewer) = app.ui.panel.attachment.as_ref().filter(|(open, _)| open == id)?;
    Some(viewer.view(app, true, super::files::wrap_on(app), t))
}

fn open(mode: Mode, url: &str, name: &str) -> Result<Content, String> {
    match mode {
        Mode::Pdf => return Ok(Content::Browser(Frame::new(mode, url.to_owned()))),
        Mode::Html => return Ok(Content::Browser(Frame::new(mode, html_url(url, name)))),
        Mode::Audio | Mode::Video => return Ok(Content::Browser(Frame::new(mode, media_url(mode, url, name)))),
        _ => {}
    }
    let (max, limit) = mode.limit();
    let bytes = download(url, max, limit)?;
    Ok(match mode {
        Mode::Large(delimiter) => Content::Large(LargeText::new(bytes, delimiter)?),
        Mode::Archive => Content::Archive(Archive::open(fp::file_name(name), bytes)?),
        Mode::Sqlite => Content::Sqlite(Database::open(&bytes)?),
        Mode::Font => Content::Browser(Frame::new(mode, font_url(&bytes, name))),
        Mode::Notebook => Content::Notebook(notebook::NotebookView::new(&bytes)?),
        Mode::Office(format) => Content::Office(office::Office::open(format, &bytes)?),
        _ => unreachable!(),
    })
}

fn download(url: &str, max: usize, limit: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url).timeout(Duration::from_secs(30)).call()
        .map_err(|_| "The file could not be loaded. Close it and try again.")?;
    let expected = response.header("content-length").and_then(|s| s.parse::<usize>().ok());
    if expected.is_some_and(|size| size > max) { return Err(limit.to_owned()) }
    let mut bytes = Vec::new();
    response.into_reader().take(max as u64 + 1).read_to_end(&mut bytes).map_err(|_| "The file could not be read.")?;
    if bytes.len() > max { return Err(limit.to_owned()) }
    if expected.is_some_and(|size| size != bytes.len()) { return Err("The file download was incomplete. Close it and try again.".into()) }
    Ok(bytes)
}

struct Job<A>(Arc<Mutex<Option<Result<A, String>>>>);

impl<A: Send + 'static> Job<A> {
    fn spawn(waker: Option<rune::Waker>, work: impl FnOnce() -> Result<A, String> + Send + 'static) -> Self {
        let result = Arc::new(Mutex::new(None));
        let weak = Arc::downgrade(&result);
        std::thread::spawn(move || {
            let value = work();
            if let Some(result) = weak.upgrade() {
                *result.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
                if let Some(wake) = waker { wake() }
            }
        });
        Self(result)
    }

    fn take(&self) -> Option<Result<A, String>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

struct TempAsset {
    dir: PathBuf,
    path: PathBuf,
}

impl TempAsset {
    fn write(name: &str, bytes: &[u8]) -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("s5-file-preview-{}", s5_net::uuid()));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder.create(&dir).map_err(|_| "Could not prepare file preview.")?;
        let asset = Self { path: dir.join(name), dir };
        std::fs::write(&asset.path, bytes).map_err(|_| "Could not prepare file preview.")?;
        Ok(asset)
    }

    fn path(&self) -> &Path { &self.path }
}

impl Drop for TempAsset {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn run(program: &str, flags: &[&str], path: &Path, args: &[&str], max: usize) -> Result<Vec<u8>, String> {
    let temp = TempAsset::write("output", &[])?;
    let output = std::fs::File::options().write(true).open(temp.path()).map_err(|_| "Could not prepare file preview.")?;
    let mut child = Command::new(program).args(flags).arg(path).args(args)
        .env("LC_ALL", "C").stdin(Stdio::null()).stdout(output).stderr(Stdio::null())
        .spawn().map_err(|e| format!("{program} is unavailable: {e}"))?;
    let start = Instant::now();
    loop {
        let size = std::fs::metadata(temp.path()).map_or(0, |f| f.len());
        if size > max as u64 || start.elapsed() > Duration::from_secs(20) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("This file took too long or was too large to preview.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return Err("This file could not be opened. It may be damaged or unsupported.".into()),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Could not read the file preview.".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(30)),
        }
    }
    let mut bytes = Vec::new();
    std::fs::File::open(temp.path()).map_err(|_| "Could not read the file preview.")?.take(max as u64 + 1)
        .read_to_end(&mut bytes).map_err(|_| "Could not read the file preview.")?;
    if bytes.len() > max { return Err("This file is too large to preview.".into()) }
    Ok(bytes)
}

struct Frame {
    mode: Mode,
    url: String,
    live: Option<Live>,
    still: Option<rune::Pixels>,
    size: [u32; 2],
    error: Option<String>,
}

impl Frame {
    fn new(mode: Mode, url: String) -> Self {
        Self { mode, url, live: None, still: None, size: [0; 2], error: None }
    }

    fn poll(&mut self) -> bool {
        let Some(live) = &self.live else { return false };
        if live.gone() {
            self.error = Some("Unable to load file preview.".into());
            self.live = None;
            return true;
        }
        if live.messages().iter().any(|message| message == "file-preview-error") {
            let message = match self.mode {
                Mode::Audio => "Unable to load audio.",
                Mode::Video => "Video unavailable",
                Mode::Font => "The font is damaged or unsupported.",
                _ => "Unable to load file preview.",
            };
            self.error = Some(message.into());
            return true;
        }
        false
    }

    fn view(&self, app: &App, t: &T) -> El {
        if let Some(error) = &self.error { return failure(error, t) }
        let title = shown_name(app).to_owned();
        let dark = t.dark;
        let layer = col().key("file-preview-browser").grow(1.0).min_w(0.0).min_h(0.0).label(title)
            .on_layout_tx(move |app: &mut App, rect, tx| {
                if !layout(app, [rect[2], rect[3]], dark) { tx.live_only() }
            });
        if let Some(pixels) = &self.still {
            return layer.child(el().w_full().grow(1.0).min_h(0.0).pixels_fit(pixels, rune::SurfaceFit::Fit));
        }
        if let Some(live) = &self.live {
            return layer.child(super::html_frame::interactive_surface(live, format!("file-preview-live-{}", live.id), |_| {}, |_| {}, true));
        }
        layer.child(loading(t))
    }
}

fn layout(app: &mut App, size: [f32; 2], dark: bool) -> bool {
    let size = size.map(|value| value.round() as u32);
    if size.contains(&0) { return false }
    let Some(Content::Browser(frame)) = active_mut(app) else { return false };
    if frame.size == size || frame.still.is_some() || frame.error.is_some() { return false }
    frame.size = size;
    if let Some(live) = &frame.live {
        let id = live.id;
        super::preview::inline_frame_cmd(app, Cmd::LiveResize(id, size[0], size[1]));
    } else {
        let url = frame.url.clone();
        let is_pdf = matches!(frame.mode, Mode::Pdf);
        let mut live = Live::new(url, size[0], size[1], dark);
        live.file_pdf = is_pdf;
        live.script = Some("addEventListener('error', event => { if (event.target instanceof HTMLMediaElement) window.__s5Frame?.('file-preview-error'); }, true); addEventListener('DOMContentLoaded', () => { document.fonts?.load('24px preview').catch(() => window.__s5Frame?.('file-preview-error')); });".into());
        let live = super::preview::inline_frame_cmd(app, Cmd::LiveOpen(live.clone())).then_some(live);
        if let Some(Content::Browser(frame)) = active_mut(app) {
            frame.live = live;
            if frame.live.is_none() {
                frame.error = Some("Unable to load file preview. Chrome is unavailable.".into());
            }
        }
    }
    true
}

fn escape(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

fn html_url(url: &str, title: &str) -> String {
    let url = escape(url);
    let title = escape(title);
    super::html_frame::data_url(&format!(r#"<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; frame-src https: http: data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<style>html,body{{margin:0;height:100%;color-scheme:light dark}}iframe{{display:block;width:100%;height:100%;border:0}}</style>
<iframe sandbox="allow-scripts allow-forms allow-popups allow-downloads" src="{url}" title="{title}"></iframe>"#))
}

fn media_url(mode: Mode, url: &str, name: &str) -> String {
    let tag = if matches!(mode, Mode::Video) { "video" } else { "audio" };
    let escaped = escape(url);
    let name = escape(name);
    super::html_frame::data_url(&format!(r#"<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; media-src https: http: data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<style>html{{color-scheme:light dark;font:12px system-ui}}body{{margin:0;min-height:100dvh;box-sizing:border-box;display:grid;place-items:center;padding:16px}}
video{{max-width:100%;max-height:calc(100dvh - 32px);background:black}}audio{{width:min(100%,560px)}}small{{padding-bottom:8px}}section{{display:grid;justify-items:center;width:100%}}</style>
<section>{}<{tag} src="{escaped}" aria-label="{name}" controls playsinline preload="metadata"></{tag}></section>"#,
        if matches!(mode, Mode::Audio) { format!("<small>{name}</small>") } else { String::new() }))
}

fn font_url(bytes: &[u8], name: &str) -> String {
    use base64::Engine;
    let mime = match name.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "otf" => "font/otf", "woff" => "font/woff", "woff2" => "font/woff2", _ => "font/ttf",
    };
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    let samples: String = [12, 16, 24, 40, 64].iter().map(|size| format!(r#"<p style="font-size:{size}px"><small>{size} px</small>The quick brown fox jumps over the lazy dog.</p>"#)).collect();
    super::html_frame::data_url(&format!(r#"<!doctype html><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; font-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<style>@font-face{{font-family:preview;src:url('data:{mime};base64,{data}')}}html{{color-scheme:light dark}}body{{margin:0;padding:32px;font-family:preview;overflow-wrap:anywhere}}h1{{font-size:40px;font-weight:normal}}p{{line-height:1.4}}small{{font:12px monospace;display:block;opacity:.6;margin-bottom:8px}}</style>
<h1>Font preview</h1><p style="font-size:24px">ABCDEFGHIJKLMNOPQRSTUVWXYZ<br>abcdefghijklmnopqrstuvwxyz<br>0123456789 !?&amp;@#%(){{}}[]</p>{samples}"#))
}

fn loading(t: &T) -> El {
    col().grow(1.0).min_h(0.0).center().child(kit::Spinner::new().size("lg").tone("muted").build(&kit::theme(t)))
}

fn failure(error: &str, t: &T) -> El {
    col().grow(1.0).min_h(0.0).center().gap(8.0).padding(20.0).role("alert")
        .child(text(error.to_owned()).font(theme::TEXT_XS).color(t.c.destructive).wrap())
        .child(kit::Button::new("Try again").variant("outline").size("xs")
            .on_press(super::files::refresh).build(t))
}

fn bar(_: &T) -> El { row().w_full().min_h(40.0).px(12.0).py(4.0).gap(8.0).center_y() }
fn divider(t: &T) -> El { el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)) }
fn note(value: impl Into<String>, t: &T) -> El { text(value.into()).font(theme::TEXT_XS).color(t.c.muted_foreground) }
fn action(label: &str, glyph: &'static rune::Icon, disabled: bool, t: &T) -> El {
    kit::icon_button(glyph, label).size("icon-xs").variant("ghost").disabled(disabled).build(t).help(label.to_owned())
}
fn grouped(n: u64) -> String { super::files::grouped(n) }

fn byte_size(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let bytes = bytes as f64;
    if bytes < KIB { format!("{bytes:.0} B") }
    else if bytes < KIB * KIB { format!("{:.1} KB", bytes / KIB) }
    else { format!("{:.1} MB", bytes / (KIB * KIB)) }
}

fn short_date(seconds: i64) -> Option<String> {
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let day_of_year = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (0..=9999).contains(&year).then(|| format!("{month}/{day}/{year}"))
}

fn image_pixels(bytes: &[u8]) -> Result<rune::Pixels, String> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().map_err(|_| "Could not read image.")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().map_err(|_| "Could not decode image.")?.to_rgba8();
    let pixels = rune::Pixels::new();
    pixels.set(rgba.width(), rgba.height(), rgba.into_raw());
    Ok(pixels)
}

pub(super) fn show_more_cells(app: &mut App) {
    if let Some(Content::Notebook(notebook)) = active_mut(app) {
        notebook.show_more();
    }
}
