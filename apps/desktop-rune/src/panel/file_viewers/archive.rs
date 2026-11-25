use std::collections::BTreeMap;
use std::io::{Cursor, Read};
use std::ops::Range;
use std::sync::Arc;

use rune::{El, Pixels, ScrollSpec, col, el, icon, text};

use super::{Content, Job, action, bar, byte_size, divider, loading, note};
use crate::panel::file_preview as fp;
use crate::theme::{self, T};
use crate::{App, icons};

const FILE_LIMIT: usize = 10 * 1024 * 1024;
const ENTRY_LIMIT: usize = 2048;
const RATIO_LIMIT: usize = 100;

pub(super) struct Archive {
    source: Arc<Input>,
    folder: String,
    selected: Option<String>,
    result: Option<Result<EntryView, String>>,
    job: Option<Job<EntryView>>,
    size: u64,
}

struct Input {
    entries: Vec<Entry>,
    bytes: Vec<u8>,
}

struct Entry {
    path: String,
    dir: bool,
    size: u64,
    modified: Option<String>,
    location: Location,
}

enum Location {
    Zip(usize),
    Bytes(Range<usize>),
}

enum EntryView {
    Text(String, bool),
    Image(Pixels),
}

fn path(raw: &str) -> Option<String> {
    if raw.len() > 1024 || raw.starts_with(['/', '\\']) || raw.contains(['\0', '\\', ':'])
        || raw.split('/').any(|part| part == "..")
    {
        return None;
    }
    let clean = raw.split('/').filter(|part| !matches!(*part, "" | ".")).collect::<Vec<_>>().join("/");
    (!clean.is_empty()).then_some(clean)
}

fn zip_entries(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| "This ZIP archive could not be opened.")?;
    if zip.len() > ENTRY_LIMIT {
        return Err("This archive contains more than 2,048 entries.".into());
    }
    let mut entries = Vec::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let file = zip.by_index(i).map_err(|_| "An archive entry is damaged or encrypted.")?;
        let size = file.size();
        total = total.saturating_add(size);
        if total > super::BINARY_LIMIT as u64 || size > file.compressed_size().saturating_mul(RATIO_LIMIT as u64) {
            return Err("The expanded archive exceeds the preview size or compression ratio limit.".into());
        }
        let name = file.name().map_err(|_| "An archive filename could not be decoded.")?;
        let Some(path) = path(&name) else { continue };
        if file.is_symlink() {
            continue;
        }
        let modified = file.last_modified().map(|date| format!("{}/{}/{}", date.month(), date.day(), date.year()));
        entries.push(Entry { path, dir: file.is_dir(), size, modified, location: Location::Zip(i) });
    }
    Ok(entries)
}

fn tar_text(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or_default()
}

fn octal(bytes: &[u8]) -> Result<usize, String> {
    let value = tar_text(bytes).trim();
    if value.is_empty() { Ok(0) } else { usize::from_str_radix(value, 8).map_err(|_| "This TAR uses an unsupported numeric format.".into()) }
}

fn pax(bytes: &[u8]) -> Result<BTreeMap<String, String>, String> {
    let invalid = || "The TAR metadata is damaged.".to_owned();
    if bytes.len() > 65_536 {
        return Err("Archive metadata is too large.".into());
    }
    let mut attributes = BTreeMap::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let span = bytes[start..].iter().take(11).position(|b| *b == b' ').ok_or_else(invalid)?;
        let length = tar_text(&bytes[start..start + span]).parse::<usize>().map_err(|_| invalid())?;
        let end = start.checked_add(length).filter(|end| *end <= bytes.len() && *end > start + span + 2).ok_or_else(invalid)?;
        let record = bytes.get(start + span + 1..end).and_then(|r| r.strip_suffix(b"\n")).ok_or_else(invalid)?;
        let (key, value) = tar_text(record).split_once('=').ok_or_else(invalid)?;
        if matches!(key, "path" | "size" | "mtime") {
            attributes.insert(key.to_owned(), value.to_owned());
        }
        start = end;
    }
    Ok(attributes)
}

fn tar_entries(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let damaged = || "An archive entry is too large or damaged.".to_owned();
    let mut offset = 0;
    let mut entries = Vec::new();
    let (mut global, mut local) = (BTreeMap::new(), BTreeMap::new());
    let mut long_name = None;
    while let Some(header) = bytes.get(offset..offset + 512) {
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let checksum: usize = header.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as usize }).sum();
        if checksum != octal(&header[148..156])? {
            return Err("The TAR header is damaged.".into());
        }
        let kind = header[156];
        let field = |key| local.get(key).or_else(|| global.get(key)).map(String::as_str);
        let size = if matches!(kind, b'L' | b'x' | b'g') {
            octal(&header[124..136])?
        } else if let Some(size) = field("size") {
            size.parse::<usize>().map_err(|_| damaged())?
        } else {
            octal(&header[124..136])?
        };
        let start = offset + 512;
        let end = start.checked_add(size).filter(|end| *end <= bytes.len()).ok_or_else(damaged)?;
        let content = &bytes[start..end];
        match kind {
            b'L' => long_name = Some(tar_text(content).to_owned()),
            b'x' => local = pax(content)?,
            b'g' => {
                for (key, value) in pax(content)? {
                    if value.is_empty() { global.remove(&key); } else { global.insert(key, value); }
                }
            }
            _ => {
                let name = long_name.take();
                let prefix = if &header[257..265] == b"ustar\x0000" { tar_text(&header[345..500]) } else { "" };
                let short = tar_text(&header[..100]);
                let joined = if prefix.is_empty() { short.to_owned() } else { format!("{prefix}/{short}") };
                let raw_path = field("path").or(name.as_deref()).unwrap_or(&joined);
                if matches!(kind, 0 | b'0' | b'5') && let Some(path) = path(raw_path) {
                    if entries.len() >= ENTRY_LIMIT {
                        return Err("This archive contains more than 2,048 entries.".into());
                    }
                    let seconds = field("mtime").and_then(|v| v.split('.').next()?.parse::<i64>().ok())
                        .or_else(|| octal(&header[136..148]).ok().and_then(|v| i64::try_from(v).ok()));
                    let modified = seconds.and_then(|sec| super::short_date(sec));
                    entries.push(Entry { path, dir: kind == b'5', size: size as u64, modified, location: Location::Bytes(start..end) });
                }
                local.clear();
            }
        }
        offset = start + size.div_ceil(512) * 512;
    }
    Ok(entries)
}

impl Input {
    fn read(&self, path: &str) -> Result<EntryView, String> {
        let entry = self.entries.iter().find(|e| e.path == path && !e.dir).ok_or("This archive entry is unavailable.")?;
        if entry.size as usize > FILE_LIMIT {
            return Err("Entry previews are limited to 10 MB.".into());
        }
        let mut extracted = Vec::new();
        let bytes = match &entry.location {
            Location::Zip(index) => {
                let mut zip = zip::ZipArchive::new(Cursor::new(&self.bytes)).map_err(|_| "This ZIP archive could not be opened.")?;
                zip.by_index(*index).map_err(|_| "This archive entry is unavailable.")?.take(FILE_LIMIT as u64 + 1).read_to_end(&mut extracted)
                    .map_err(|_| "The archive entry could not be read.")?;
                if extracted.len() > FILE_LIMIT { return Err("Entry previews are limited to 10 MB.".into()) }
                extracted.as_slice()
            }
            Location::Bytes(range) => &self.bytes[range.clone()],
        };
        match fp::kind(&entry.path) {
            fp::Kind::Image => super::image_pixels(bytes).map(EntryView::Image),
            fp::Kind::Text | fp::Kind::Markdown | fp::Kind::Mermaid | fp::Kind::Html | fp::Kind::Table(_) if !bytes.contains(&0) => {
                let source = std::str::from_utf8(bytes).map_err(|_| "Only UTF-8 text and images can be opened inside an archive.")?;
                let max = source.floor_char_boundary(source.len().min(1024 * 1024));
                Ok(EntryView::Text(source[..max].to_owned(), max < source.len()))
            }
            _ => Err("Only UTF-8 text and images can be opened inside an archive.".into()),
        }
    }
}

#[derive(Default)]
struct Child {
    dir: bool,
    size: u64,
    modified: Option<String>,
}

fn children(entries: &[Entry], folder: &str) -> Vec<(String, Child)> {
    let mut out = BTreeMap::<String, Child>::new();
    for entry in entries {
        let Some(rest) = entry.path.strip_prefix(folder).filter(|p| !p.is_empty()) else { continue };
        let (name, nested) = rest.split_once('/').map_or((rest, false), |(name, _)| (name, true));
        let child = out.entry(name.to_owned()).or_default();
        child.dir |= entry.dir || nested;
        child.size += entry.size;
        if child.modified.is_none() { child.modified = entry.modified.clone(); }
    }
    let mut out: Vec<_> = out.into_iter().collect();
    out.sort_by_key(|(_, item)| !item.dir);
    out
}

impl Archive {
    pub(super) fn open(name: &str, mut bytes: Vec<u8>) -> Result<Self, String> {
        let lower = name.to_ascii_lowercase();
        let size = bytes.len() as u64;
        let entries = if bytes.starts_with(b"PK") || lower.ends_with(".zip") || lower.ends_with(".jar") {
            zip_entries(&bytes)?
        } else if bytes.starts_with(&[0x1f, 0x8b]) || lower.ends_with(".gz") || lower.ends_with(".tgz") {
            let max = super::BINARY_LIMIT.min(bytes.len() * RATIO_LIMIT);
            let mut expanded = Vec::new();
            flate2::read::MultiGzDecoder::new(&bytes[..]).take(max as u64 + 1).read_to_end(&mut expanded)
                .map_err(|_| "This compressed archive could not be opened.")?;
            if expanded.len() > max {
                return Err("The expanded archive exceeds the preview size or compression ratio limit.".into());
            }
            bytes = expanded;
            if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
                tar_entries(&bytes)?
            } else {
                vec![Entry { path: name[..name.len().saturating_sub(if lower.ends_with(".gz") { 3 } else { 0 })].to_owned(),
                    dir: false, size: bytes.len() as u64, modified: None, location: Location::Bytes(0..bytes.len()) }]
            }
        } else {
            tar_entries(&bytes)?
        };
        if entries.is_empty() { return Err("This archive has no readable entries.".into()) }
        Ok(Self { source: Arc::new(Input { entries, bytes }), folder: String::new(), selected: None, result: None, job: None, size })
    }

    pub(super) fn poll(&mut self) -> bool {
        if let Some(result) = self.job.as_ref().and_then(Job::take) {
            self.result = Some(result);
            self.job = None;
            return true;
        }
        false
    }

    pub(super) fn view(&self, t: &T) -> El {
        let go_back = if self.selected.is_some() { "Back to archive" } else { "Parent folder" };
        let location = self.selected.as_deref().unwrap_or(if self.folder.is_empty() { "Archive contents" } else { &self.folder });
        let header = bar(t).child(action(go_back, &icons::CHEVRON_LEFT, self.folder.is_empty() && self.selected.is_none(), t).on_press(back))
            .child(note(location.to_owned(), t).grow(1.0).truncate())
            .child(note(format!("{} entries · {}", self.source.entries.len(), byte_size(self.size)), t));
        let body = if self.selected.is_some() {
            match &self.result {
                None => loading(t),
                Some(Err(error)) => super::failure(error, t),
                Some(Ok(EntryView::Image(pixels))) => col().grow(1.0).min_h(0.0).padding(16.0).child(el().w_full().grow(1.0).min_h(0.0).pixels_fit(pixels, rune::SurfaceFit::Fit)),
                Some(Ok(EntryView::Text(source, cut))) => col().grow(1.0).min_h(0.0)
                    .children(cut.then(|| bar(t).child(note("Preview limited to the first 1 MB.", t))))
                    .child(scroller().child(text(source.clone()).font(theme::MONO_XS).color(t.c.foreground).padding(12.0).wrap_anywhere().selectable(true))),
            }
        } else {
            let children = children(&self.source.entries, &self.folder);
            let mut table = col().w_full().child(bar(t)
                .child(note("Name", t).grow(1.0)).child(note("Size", t).w(64.0)).child(note("Modified", t).w(72.0))).child(divider(t));
            if children.is_empty() { table = table.child(note("This folder is empty.", t).padding(24.0)); }
            for (name, child) in children {
                let path = format!("{}{name}", self.folder);
                table = table.child(bar(t).min_h(32.0).hover(t.c.accent).role("button").label(name.clone()).focusable(true)
                    .cursor(rune::Cursor::Pointer).on_press(move |app: &mut App| open_entry(app, &path, child.dir))
                    .child(icon(if child.dir { &icons::FOLDER } else { &icons::FILE }, 16.0).color(t.c.muted_foreground))
                    .child(text(name).font(theme::TEXT_XS).color(t.c.foreground).grow(1.0).truncate())
                    .child(note(byte_size(child.size), t).w(64.0))
                    .child(note(child.modified.unwrap_or_else(|| "-".into()), t).w(72.0))).child(divider(t));
            }
            scroller().child(table)
        };
        col().grow(1.0).min_h(0.0).child(header).child(divider(t)).child(body)
    }
}

fn scroller() -> El {
    col().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })
}

fn back(app: &mut App) {
    let Some(Content::Archive(archive)) = super::active_mut(app) else { return };
    archive.result = None;
    archive.job = None;
    if archive.selected.take().is_none() {
        archive.folder = archive.folder.trim_end_matches('/').rsplit_once('/').map_or(String::new(), |(f, _)| format!("{f}/"));
    }
}

fn open_entry(app: &mut App, path: &str, directory: bool) {
    let wake = app.ui.panel.waker.clone();
    let Some(Content::Archive(archive)) = super::active_mut(app) else { return };
    if directory {
        archive.folder = format!("{path}/");
        return;
    }
    archive.selected = Some(path.to_owned());
    archive.result = None;
    let (input, path) = (archive.source.clone(), path.to_owned());
    archive.job = Some(Job::spawn(wake, move || input.read(&path)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_navigation_rolls_up_implicit_folders_within_the_root() {
        let entry = |path: &str, size| Entry { path: path.into(), dir: false, size, modified: None, location: Location::Bytes(0..0) };
        let entries = [entry("docs/readme.md", 5), entry("docs/folder/a.txt", 10), entry("file.txt", 7)];
        let rows = children(&entries, "");
        assert_eq!((rows[0].0.as_str(), rows[0].1.size, rows[0].1.dir), ("docs", 15, true));
        let docs = children(&entries, "docs/");
        assert_eq!(docs.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>(), ["folder", "readme.md"]);
        assert_eq!(path("../unsafe.txt"), None);
        assert_eq!(path("./folder/notes"), Some("folder/notes".into()));
    }
}
