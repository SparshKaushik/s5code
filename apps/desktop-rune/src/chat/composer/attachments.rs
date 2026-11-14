//! Files queued in a draft and their trip to the server (useComposerAttachmentIntake): supported
//! images inline through `assets.persistChatAttachments`, anything else through a signed upload
//! (`attachments.createUploadUrl`, then the bytes).

use std::path::Path;

use serde_json::{Value, json};

use super::Outgoing;

/// Image types the server stores (PROVIDER_SEND_TURN_SUPPORTED_IMAGE_MIME_TYPES), by extension.
const IMAGE_TYPES: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];
const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
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

fn image_type(name: &str) -> Option<&'static str> {
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

/// Takes `paths` into `attachments`, keeping the ones that can be sent; the last refusal is returned.
pub fn intake(attachments: &mut Vec<String>, paths: impl IntoIterator<Item = String>) -> Option<String> {
    let mut error = None;
    for path in paths {
        match check(&path) {
            Ok(_) => attachments.push(path),
            Err(e) => error = Some(e),
        }
    }
    error
}

pub struct FileUpload {
    path: String,
    name: String,
    mime: &'static str,
    size: u64,
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
        return Ok(Staged::Image(json!({
            "type": "image", "name": name, "mimeType": mime,
            "sizeBytes": bytes.len(), "dataUrl": format!("data:{mime};base64,{data}"),
        })));
    }
    let short: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let short = if short.trim().is_empty() { "file".to_owned() } else { short.trim().to_owned() };
    Ok(Staged::File(FileUpload { path: path.to_owned(), mime: mime_type(&name), name: short, size }))
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
                let request = net.request("attachments.createUploadUrl", json!({"type": "file", "name": f.name, "mimeType": f.mime, "sizeBytes": f.size}));
                files.push((f, Step::Url(request)));
            }
        }
    }
    let images = if images.is_empty() {
        Ok(Vec::new())
    } else {
        Err(net.request("assets.persistChatAttachments", json!({"threadId": thread_id, "messageId": message_id, "attachments": images})))
    };
    Upload { payload, launch, outgoing, images, files }
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
        out.push(json!({"type": "file", "id": id, "name": file.name, "mimeType": file.mime, "sizeBytes": file.size}));
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
