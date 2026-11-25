//! Files queued in a draft and their trip to the server (useComposerAttachmentIntake): supported
//! images inline through `assets.persistChatAttachments`, anything else through a signed upload
//! (`attachments.createUploadUrl`, then the bytes).

use std::path::Path;

use serde_json::{Value, json};

use super::Outgoing;

/// Image types the server stores (PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES), by extension.
const IMAGE_TYPES: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];
pub(super) const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
/// PROVIDER_SEND_TURN_MAX_FILE_BYTES.
pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;
const MAX_NAME_CHARS: usize = 255;

/// Content types for common extensions; anything else is `application/octet-stream`.
const FILE_TYPES: &[(&str, &str)] = &[
    ("txt", "text/plain"),
    ("log", "text/plain"),
    ("md", "text/markdown"),
    ("csv", "text/csv"),
    ("tsv", "text/tab-separated-values"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "text/javascript"),
    ("mjs", "text/javascript"),
    ("ts", "text/plain"),
    ("tsx", "text/plain"),
    ("rs", "text/plain"),
    ("py", "text/x-python"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("yaml", "application/yaml"),
    ("yml", "application/yaml"),
    ("toml", "application/toml"),
    ("pdf", "application/pdf"),
    ("zip", "application/zip"),
    ("gz", "application/gzip"),
    ("tar", "application/x-tar"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ("pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation"),
    ("ipynb", "application/x-ipynb+json"),
    ("svg", "image/svg+xml"),
    ("dng", "image/x-adobe-dng"),
    ("mp4", "video/mp4"),
    ("mov", "video/quicktime"),
    ("webm", "video/webm"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
];

fn extension(name: &str) -> Option<String> {
    Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase)
}

pub(super) fn image_type(name: &str) -> Option<&'static str> {
    let ext = extension(name)?;
    IMAGE_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)
}

/// A type `assets.persistChatAttachments` stores as an image.
pub fn is_image(name: &str) -> bool {
    image_type(name).is_some()
}

pub fn mime_type(name: &str) -> &'static str {
    let ext = extension(name).unwrap_or_default();
    image_type(name).or_else(|| FILE_TYPES.iter().find(|(e, _)| *e == ext).map(|(_, m)| *m)).unwrap_or("application/octet-stream")
}

pub fn file_name(path: &str) -> String {
    Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| path.to_owned())
}

/// formatAttachmentSize: whole KB, or MB to one place once it rounds to 1024 KB.
pub fn format_size(bytes: u64) -> String {
    let kb = bytes.div_ceil(1024).max(1);
    if kb >= 1024 { format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0)) } else { format!("{kb} KB") }
}

fn unreadable(name: &str) -> String {
    format!("'{name}' is empty or could not be read.")
}

fn too_large(name: &str) -> String {
    format!("'{name}' exceeds the 50 MB attachment limit.")
}

/// A file's size, or why it cannot be attached.
pub fn check(path: &str) -> Result<u64, String> {
    let name = file_name(path);
    let size = std::fs::metadata(path).ok().filter(|m| m.is_file()).map(|m| m.len()).filter(|n| *n > 0).ok_or_else(|| unreadable(&name))?;
    if size > MAX_FILE_BYTES {
        return Err(too_large(&name));
    }
    Ok(size)
}

/// PROVIDER_SEND_TURN_MAX_ATTACHMENTS.
pub const MAX_ATTACHMENTS: usize = 100;

/// A queued file gone from disk since it was added or stashed (composerFileNeedsReattach).
pub fn needs_reattach(path: &str) -> bool {
    !Path::new(path).exists()
}

/// Where a queued file stands (ComposerAttachmentFileList): ready at its size, gone and waiting
/// to be attached again, or present but unsendable.
#[derive(Debug, PartialEq)]
pub enum Status {
    Ready(u64),
    Reattach,
    Remove(String),
}

pub fn status(path: &str) -> Status {
    if needs_reattach(path) {
        return Status::Reattach;
    }
    match check(path) {
        Ok(size) => Status::Ready(size),
        Err(e) => Status::Remove(e),
    }
}

/// The note beside a queued file: its size, "Attach again" or "Remove to send".
pub fn status_label(status: &Status) -> String {
    match status {
        Status::Ready(size) => format_size(*size),
        Status::Reattach => "Attach again".to_owned(),
        Status::Remove(_) => "Remove to send".to_owned(),
    }
}

/// Why Send waits on the draft's files (attachmentBlockReason): some are gone from disk.
pub fn block_reason(paths: &[String]) -> Option<&'static str> {
    match paths.iter().filter(|p| needs_reattach(p)).count() {
        0 => None,
        1 => Some("Attach the interrupted file again or remove it"),
        _ => Some("Attach the interrupted files again or remove them"),
    }
}

/// Takes `paths` into `attachments`, keeping the ones that can be sent; the last refusal is
/// returned. A pick named like a file gone from disk takes that file's place.
pub fn intake(attachments: &mut Vec<String>, paths: impl IntoIterator<Item = String>) -> Option<String> {
    let mut error = None;
    for path in paths {
        let name = file_name(&path);
        let marker = attachments.iter().position(|a| needs_reattach(a) && file_name(a) == name);
        if marker.is_none() && attachments.len() >= MAX_ATTACHMENTS {
            error = Some(format!("You can attach up to {MAX_ATTACHMENTS} files per message."));
            continue;
        }
        if let Err(e) = check(&path) {
            error = Some(e);
            continue;
        }
        match marker {
            Some(at) => attachments[at] = path,
            None => attachments.push(path),
        }
    }
    error
}

/// PASTED_TEXT_ATTACHMENT_THRESHOLD_BYTES.
pub const PASTED_TEXT_THRESHOLD: usize = 32 * 1024;

/// pastedTextDisposition: a paste of 32 KiB or more, or one that would push the prompt over its
/// limit, becomes a text file, unless pasted as text (Mod+Shift+V). UTF-8 bytes are never fewer
/// than web's UTF-16 length, so the byte count covers both of web's checks.
pub fn folds_paste(text: &str, as_text: bool, exceeds_limit: bool) -> bool {
    !as_text && !text.is_empty() && (exceeds_limit || text.len() >= PASTED_TEXT_THRESHOLD)
}

/// nextPastedTextFileName: `pasted-text.txt`, then `pasted-text-2.txt`, ... unused among `names`.
pub fn pasted_text_name<'a>(names: impl IntoIterator<Item = &'a str>) -> String {
    let taken: std::collections::HashSet<String> = names.into_iter().map(str::to_lowercase).collect();
    std::iter::once("pasted-text.txt".to_owned())
        .chain((2..).map(|n| format!("pasted-text-{n}.txt")))
        .find(|name| !taken.contains(name))
        .unwrap_or_default()
}

/// Writes pasted text to a file named `name` in a folder of its own, so the name is kept.
pub fn write_pasted(name: &str, text: &str) -> Option<String> {
    let dir = std::env::temp_dir().join(format!("s5-pasted-{}", s5_net::uuid()));
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(name);
    std::fs::write(&path, text).ok()?;
    Some(path.display().to_string())
}

/// A queued image's tile is 64 points; it decodes to that on a 2x display, never the original.
const THUMB_DECODE: u32 = 128;
/// Images larger than this on disk show their name instead of a thumbnail.
const MAX_THUMB_SOURCE: u64 = 20 * 1024 * 1024;

/// A queued image's thumbnail (ComposerImageThumbnail), decoded off the UI thread.
pub struct Thumb {
    pub pixels: rune::Pixels,
    broken: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// A view has seen it decoded.
    shown: bool,
    /// A snapshot's `SnapShotSource`, read once from beside the picture.
    pub source: Option<serde_json::Value>,
}

impl Thumb {
    pub fn failed(&self) -> bool {
        self.broken.load(std::sync::atomic::Ordering::Relaxed)
    }
}

fn decode_thumb(path: &str, side: u32) -> Option<image::RgbaImage> {
    let size = std::fs::metadata(path).ok()?.len();
    if size == 0 || size > MAX_THUMB_SOURCE {
        return None;
    }
    let image = image::load_from_memory(&std::fs::read(path).ok()?).ok()?;
    Some(image.thumbnail(side, side).to_rgba8())
}

fn load_thumb(path: &str, waker: Option<rune::Waker>, now: bool) -> Thumb {
    let source = crate::desktop::capture::attached_source(path);
    // A snapshot's wider tile decodes at twice its 208 points.
    let side = if source.is_some() { 416 } else { THUMB_DECODE };
    let thumb = Thumb { pixels: rune::Pixels::new(), broken: Default::default(), shown: false, source };
    let (path, pixels, broken) = (path.to_owned(), thumb.pixels.clone(), thumb.broken.clone());
    let run = move || {
        match decode_thumb(&path, side) {
            Some(image) => pixels.set(image.width(), image.height(), image.into_raw()),
            None => broken.store(true, std::sync::atomic::Ordering::Relaxed),
        }
        if let Some(wake) = &waker {
            wake();
        }
    };
    if now {
        run();
    } else {
        std::thread::spawn(run);
    }
    thumb
}

/// Thumbnails of the images queued in drafts: views ask for them, `poll` decodes them, and one
/// whose file left every draft is dropped.
#[derive(Default)]
pub struct Thumbs {
    ready: std::collections::HashMap<String, Thumb>,
    wanted: std::cell::RefCell<Vec<String>>,
}

impl Thumbs {
    /// The thumbnail for `path`, asking for it when there is none yet.
    pub fn get(&self, path: &str) -> Option<&Thumb> {
        let thumb = self.ready.get(path);
        if thumb.is_none() {
            let mut wanted = self.wanted.borrow_mut();
            if !wanted.iter().any(|w| w == path) {
                wanted.push(path.to_owned());
            }
        }
        thumb
    }

    /// Decodes `path` before returning, for scenes that draw one frame.
    /// A prepared picture takes its original's thumbnail, so its tile stays as it is.
    pub fn rename(&mut self, from: &str, to: &str) {
        if let Some(thumb) = self.ready.remove(from) {
            self.ready.insert(to.to_owned(), thumb);
        }
    }

    pub fn load_now(&mut self, path: &str) {
        self.ready.insert(path.to_owned(), load_thumb(path, None, true));
    }

    /// Starts the wanted decodes and drops the thumbnails `keep` no longer names. True when a
    /// view should draw again: one finished, or one was dropped.
    pub fn poll(&mut self, waker: Option<rune::Waker>, keep: impl Fn(&str) -> bool) -> bool {
        for path in std::mem::take(self.wanted.get_mut()) {
            if keep(&path) && !self.ready.contains_key(&path) {
                self.ready.insert(path.clone(), load_thumb(&path, waker.clone(), false));
            }
        }
        let before = self.ready.len();
        self.ready.retain(|path, _| keep(path));
        let mut changed = self.ready.len() != before;
        for thumb in self.ready.values_mut() {
            if !thumb.shown && (thumb.pixels.size().0 > 0 || thumb.failed()) {
                thumb.shown = true;
                changed = true;
            }
        }
        changed
    }
}

pub struct FileUpload {
    path: String,
    /// Uploaded without a `type`, so the server stores it as an image.
    image: bool,
    name: String,
    mime: &'static str,
    size: u64,
    /// A snapshot's `SnapShotSource`, sent with the stored image.
    source: Option<Value>,
}

pub enum Staged {
    /// An image as `assets.persistChatAttachments` takes it, inlined as a data URL.
    Image(Value),
    File(FileUpload),
}

/// What a queued file sends as: small supported images inline, anything else uploaded.
pub fn stage(path: &str) -> Result<Staged, String> {
    use base64::Engine;
    let size = check(path)?;
    let name = file_name(path);
    if let Some(mime) = image_type(&name).filter(|_| size <= MAX_IMAGE_BYTES) {
        let bytes = std::fs::read(path).map_err(|_| unreadable(&name))?;
        let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
        let mut image = json!({
            "type": "image", "name": name, "mimeType": mime,
            "sizeBytes": bytes.len(), "dataUrl": format!("data:{mime};base64,{data}"),
        });
        if let Some(source) = crate::desktop::capture::attached_source(path) {
            image["source"] = source;
        }
        return Ok(Staged::Image(image));
    }
    Ok(Staged::File(upload(path, &name, size, false)))
}

/// Every file through a signed upload, as web sends a draft once uploads are on: the stored
/// records belong to no thread, so several launches can share them.
pub fn stage_upload(path: &str) -> Result<Staged, String> {
    let size = check(path)?;
    let name = file_name(path);
    let image = image_type(&name).is_some() && size <= MAX_IMAGE_BYTES;
    Ok(Staged::File(upload(path, &name, size, image)))
}

fn upload(path: &str, name: &str, size: u64, image: bool) -> FileUpload {
    let short: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let short = if short.trim().is_empty() { "file".to_owned() } else { short.trim().to_owned() };
    let source = image.then(|| crate::desktop::capture::attached_source(path)).flatten();
    FileUpload { path: path.to_owned(), image, mime: mime_type(name), name: short, size, source }
}

enum Step {
    Url(u64),
    Bytes(u64, String),
    Done(String),
}

/// A message waiting on its attachments: the stored images, then each file's signed upload.
pub(super) struct Upload {
    pub(super) payload: Value,
    pub(super) launch: Option<String>,
    pub(super) outgoing: Outgoing,
    /// More messages that send the same stored attachments.
    pub(super) also: Vec<(Value, Option<String>, Outgoing)>,
    images: Result<Vec<Value>, u64>,
    files: Vec<(FileUpload, Step)>,
}

pub(super) fn start(net: &mut s5_net::Net, thread_id: &str, message_id: &str, staged: Vec<Staged>, payload: Value, launch: Option<String>, outgoing: Outgoing) -> Upload {
    let mut images = Vec::new();
    let mut files = Vec::new();
    for item in staged {
        match item {
            Staged::Image(v) => images.push(v),
            Staged::File(f) => {
                let mut params = json!({"type": "file", "name": f.name, "mimeType": f.mime, "sizeBytes": f.size});
                if f.image {
                    params.as_object_mut().expect("object").remove("type");
                }
                let request = net.request("attachments.createUploadUrl", params);
                files.push((f, Step::Url(request)));
            }
        }
    }
    let images = if images.is_empty() {
        Ok(Vec::new())
    } else {
        Err(net.request("assets.persistChatAttachments", json!({"threadId": thread_id, "messageId": message_id, "attachments": images})))
    };
    Upload { payload, launch, outgoing, also: Vec::new(), images, files }
}

fn failed(name: &str, error: &Value) -> Value {
    let reason = crate::shell::palette::error_message(error, "upload failed");
    json!({"message": format!("'{name}' could not be uploaded: {reason}")})
}

/// Moves each request along: the turn's attachments once all are stored, or the first failure.
pub(super) fn advance(net: &mut s5_net::Net, u: &mut Upload) -> Result<Option<Value>, Value> {
    if let Err(request) = u.images
        && let Some(reply) = net.take_reply(request)
    {
        u.images = Ok(reply?["attachments"].as_array().cloned().unwrap_or_default());
    }
    for (file, step) in &mut u.files {
        let next = match step {
            Step::Url(request) => match net.take_reply(*request) {
                None => None,
                Some(reply) => {
                    let v = reply.map_err(|e| failed(&file.name, &e))?;
                    let (Some(relative), Some(id)) = (v["relativeUrl"].as_str(), v["attachmentId"].as_str()) else {
                        return Err(failed(&file.name, &json!({"message": "no upload URL"})));
                    };
                    Some(Step::Bytes(net.upload_file(relative, Path::new(&file.path), file.mime), id.to_owned()))
                }
            },
            Step::Bytes(request, id) => match net.take_reply(*request) {
                None => None,
                Some(reply) => {
                    reply.map_err(|e| failed(&file.name, &e))?;
                    Some(Step::Done(id.clone()))
                }
            },
            Step::Done(_) => None,
        };
        if let Some(next) = next {
            *step = next;
        }
    }
    let Ok(images) = &u.images else { return Ok(None) };
    let mut out = images.clone();
    for (file, step) in &u.files {
        let Step::Done(id) = step else { return Ok(None) };
        let kind = if file.image { "image" } else { "file" };
        let mut record = json!({"type": kind, "id": id, "name": file.name, "mimeType": file.mime, "sizeBytes": file.size});
        if let Some(source) = &file.source {
            record["source"] = source.clone();
        }
        out.push(record);
    }
    Ok(Some(Value::Array(out)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_like_the_web_chip() {
        assert_eq!(format_size(0), "1 KB");
        assert_eq!(format_size(1), "1 KB");
        assert_eq!(format_size(1536), "2 KB");
        assert_eq!(format_size(1024 * 1023), "1023 KB");
        assert_eq!(format_size(1024 * 1024 - 512), "1.0 MB");
        assert_eq!(format_size(5 * 1024 * 1024 + 300 * 1024), "5.3 MB");
    }

    #[test]
    fn mime_types_come_from_the_extension() {
        assert_eq!(mime_type("Report.PDF"), "application/pdf");
        assert_eq!(mime_type("shot.jpg"), "image/jpeg");
        assert_eq!(mime_type("notes.md"), "text/markdown");
        assert_eq!(mime_type("blob"), "application/octet-stream");
        assert_eq!(mime_type("archive.weird"), "application/octet-stream");
    }

    #[test]
    fn small_images_inline_and_other_files_upload() {
        let dir = std::env::temp_dir().join(format!("s5-upload-{}", s5_net::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("shot.PNG");
        std::fs::write(&png, [1u8, 2, 3]).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "hi").unwrap();
        let empty = dir.join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        let Ok(Staged::Image(v)) = stage(png.to_str().unwrap()) else { panic!("png inlines") };
        assert_eq!(v, json!({"type": "image", "name": "shot.PNG", "mimeType": "image/png", "sizeBytes": 3, "dataUrl": "data:image/png;base64,AQID"}));
        let Ok(Staged::File(f)) = stage(notes.to_str().unwrap()) else { panic!("text uploads") };
        assert_eq!((f.name.as_str(), f.mime, f.size), ("notes.txt", "text/plain", 2));
        let Ok(Staged::File(shared)) = stage_upload(png.to_str().unwrap()) else { panic!("a shared send uploads images too") };
        assert_eq!((shared.image, shared.mime), (true, "image/png"), "FAILED: an uploaded image keeps its kind");
        let Ok(Staged::File(shared)) = stage_upload(notes.to_str().unwrap()) else { panic!("text uploads") };
        assert!(!shared.image);
        let source = json!({"kind": "snap-shot", "capturedAt": "2026-11-21T10:00:00.000Z", "appName": "Safari", "windowTitle": "Docs", "accessibleText": "Docs"});
        std::fs::write(crate::desktop::capture::source_path(png.to_str().unwrap()), source.to_string()).unwrap();
        let Ok(Staged::Image(v)) = stage(png.to_str().unwrap()) else { panic!("png inlines") };
        assert_eq!(v["source"], source, "FAILED: a snapshot sends its source with the image");
        let Ok(Staged::File(shared)) = stage_upload(png.to_str().unwrap()) else { panic!("uploads") };
        assert_eq!(shared.source, Some(source), "FAILED: an uploaded snapshot keeps its source");
        assert_eq!(stage(empty.to_str().unwrap()).err().unwrap(), "'empty.txt' is empty or could not be read.");
        assert_eq!(stage(dir.join("gone.png").to_str().unwrap()).err().unwrap(), "'gone.png' is empty or could not be read.");
        assert!(stage(dir.to_str().unwrap()).is_err());

        let mut queued = Vec::new();
        let error = intake(&mut queued, [notes.display().to_string(), empty.display().to_string()]);
        assert_eq!(queued, [notes.display().to_string()]);
        assert_eq!(error.as_deref(), Some("'empty.txt' is empty or could not be read."));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_hundred_files_fill_a_message_and_a_missing_file_is_attached_again() {
        let dir = std::env::temp_dir().join(format!("s5-upload-{}", s5_net::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let notes = dir.join("notes.txt");
        std::fs::write(&notes, "hi").unwrap();
        let notes = notes.display().to_string();
        let gone = "/nonexistent/s5/notes.txt".to_owned();
        let mut queued = vec![notes.clone(); MAX_ATTACHMENTS - 1];
        queued.push(gone.clone());
        assert_eq!(status(&gone), Status::Reattach);
        assert_eq!(status_label(&status(&gone)), "Attach again");
        assert_eq!(status_label(&Status::Remove("x".into())), "Remove to send");
        assert_eq!(block_reason(&queued), Some("Attach the interrupted file again or remove it"));
        assert_eq!(block_reason(&[gone.clone(), gone.clone()]), Some("Attach the interrupted files again or remove them"));
        // Full, but the pick replaces the missing file of the same name.
        assert_eq!(intake(&mut queued, [notes.clone()]), None);
        assert_eq!(queued.len(), MAX_ATTACHMENTS);
        assert_eq!(queued[MAX_ATTACHMENTS - 1], notes);
        assert_eq!(block_reason(&queued), None);
        assert_eq!(intake(&mut queued, [notes.clone()]).as_deref(), Some("You can attach up to 100 files per message."));
        assert_eq!(queued.len(), MAX_ATTACHMENTS);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn long_pastes_fold_into_numbered_text_files_unless_pasted_as_text() {
        let long = "x".repeat(PASTED_TEXT_THRESHOLD);
        assert!(folds_paste(&long, false, false));
        assert!(!folds_paste(&long[1..], false, false));
        assert!(folds_paste(&"\u{e9}".repeat(PASTED_TEXT_THRESHOLD / 2), false, false), "FAILED: the threshold counts bytes");
        assert!(folds_paste("short", false, true), "FAILED: a paste past the prompt limit folds");
        assert!(!folds_paste(&long, true, true), "FAILED: Mod+Shift+V pastes inline");
        assert!(!folds_paste("", false, true));
        assert_eq!(pasted_text_name(["notes.md"]), "pasted-text.txt");
        assert_eq!(pasted_text_name(["Pasted-Text.txt", "pasted-text-2.txt"]), "pasted-text-3.txt");
        let path = write_pasted("pasted-text.txt", "hello").unwrap();
        assert_eq!(file_name(&path), "pasted-text.txt");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
        std::fs::remove_dir_all(Path::new(&path).parent().unwrap()).unwrap();
    }

    #[test]
    fn files_over_50_mb_are_refused_by_name() {
        let dir = std::env::temp_dir().join(format!("s5-upload-{}", s5_net::uuid()));
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.bin");
        std::fs::File::create(&big).unwrap().set_len(MAX_FILE_BYTES + 1).unwrap();
        assert_eq!(check(big.to_str().unwrap()).err().unwrap(), "'big.bin' exceeds the 50 MB attachment limit.");
        std::fs::File::create(&big).unwrap().set_len(MAX_FILE_BYTES).unwrap();
        assert_eq!(check(big.to_str().unwrap()), Ok(MAX_FILE_BYTES));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
