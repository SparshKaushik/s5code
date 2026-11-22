//! Web's `openVsxThemes.ts` and `ThemeSearchSection`: open-source color themes from Open VSX,
//! read from the checksummed VSIX with web's ZIP-safety limits.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::vscode;
use crate::kit::{self, Button, Input};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Value, json};

const API: &str = "https://open-vsx.org/api";
const MAX_VSIX_BYTES: u64 = 20 * 1024 * 1024;
const MAX_ZIP_ENTRIES: usize = 5_000;
const MAX_UNCOMPRESSED_BYTES: u64 = 100 * 1024 * 1024;
const MAX_COMPRESSION_RATIO: u64 = 200;
const MAX_SEARCH_BYTES: u64 = 512 * 1024;
const MAX_DETAIL_BYTES: u64 = 256 * 1024;
const MAX_THEME_BYTES: u64 = 256 * 1024;
const MAX_THEMES: usize = 40;
const MAX_INCLUDE_DEPTH: usize = 8;
const MAX_THEME_FILES: usize = MAX_THEMES * MAX_INCLUDE_DEPTH;
const MAX_PATH: usize = 1024;
const MAX_COLOR: usize = 128;
const DEBOUNCE: Duration = Duration::from_millis(350);
const LICENSES: [&str; 9] = ["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "CC0-1.0", "ISC", "MIT", "MPL-2.0", "Unlicense"];
const USED_COLORS: [&str; 46] = [
    "activityBar.background", "activityBarBadge.background", "badge.background", "button.background", "button.foreground",
    "contrastBorder", "descriptionForeground", "disabledForeground", "dropdown.background", "dropdown.border",
    "editor.background", "editor.foreground", "editor.findMatchBackground", "editor.findMatchForeground",
    "editor.findMatchHighlightBackground", "editor.findMatchHighlightForeground", "editor.selectionBackground",
    "editorCursor.foreground", "editorError.foreground", "editorGroup.border", "editorPane.background",
    "editorWarning.foreground", "editorWidget.background", "errorForeground", "focusBorder", "foreground", "input.border",
    "input.placeholderForeground", "list.activeSelectionBackground", "list.hoverBackground",
    "list.inactiveSelectionBackground", "menu.background", "panel.background", "panel.border", "progressBar.background",
    "quickInput.background", "scrollbarSlider.background", "sideBar.background", "sideBar.border", "sideBar.foreground",
    "terminal.background", "terminal.foreground", "terminal.selectionBackground", "terminalCursor.foreground",
    "textCodeBlock.background", "textLink.foreground",
];
const SUGGESTIONS: [&str; 4] = ["Dracula", "Catppuccin", "Nord", "Tokyo Night"];
const SORTS: [(&str, &str); 4] = [("downloadCount", "Most downloaded"), ("rating", "Best rated"), ("timestamp", "Newest"), ("relevance", "Most relevant")];

#[derive(Clone, Debug, PartialEq)]
pub struct Extension {
    id: String,
    collection_id: String,
    namespace: String,
    extension: String,
    name: String,
    description: String,
    downloads: f64,
    source_url: Option<String>,
    manifest_url: String,
    sha256_url: String,
    vsix_url: String,
    version: String,
    license: String,
}

type Slot<O> = Arc<Mutex<Option<O>>>;
type Found = Result<Vec<Extension>, String>;

/// The search section's state; the dialog keeps it while open.
#[derive(Default)]
pub struct Search {
    query: TextEdit,
    sort: Option<&'static str>,
    sort_open: bool,
    due: Option<Instant>,
    searching: Option<Slot<Found>>,
    results: Option<Vec<Extension>>,
    error: Option<String>,
    installing: Option<(Extension, Vec<Value>, Slot<Result<Vec<Value>, String>>)>,
    confirm: Option<Extension>,
}

fn lock<O>(slot: &Arc<Mutex<O>>) -> std::sync::MutexGuard<'_, O> {
    slot.lock().unwrap_or_else(|e| e.into_inner())
}

fn spawn<O: Send + 'static>(s: &App, work: impl FnOnce() -> O + Send + 'static) -> Slot<O> {
    let slot = Arc::new(Mutex::new(None));
    let (out, waker) = (slot.clone(), s.ui.panel.waker.clone());
    std::thread::spawn(move || {
        *lock(&out) = Some(work());
        if let Some(waker) = waker {
            waker();
        }
    });
    slot
}

/// `encodeURIComponent`.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn get(url: &str, limit: u64, unavailable: &str, too_large: &str) -> Result<Vec<u8>, String> {
    let response = match ureq::get(url).timeout(Duration::from_secs(10)).call() {
        Ok(response) => response,
        Err(error) if error.to_string().contains("timed out") => return Err("Open VSX took too long to respond.".into()),
        Err(_) => return Err(unavailable.into()),
    };
    let mut bytes = Vec::new();
    response.into_reader().take(limit + 1).read_to_end(&mut bytes).map_err(|_| unavailable.to_owned())?;
    if bytes.len() as u64 > limit {
        return Err(too_large.into());
    }
    Ok(bytes)
}

/// `parseJsoncObject`.
fn object(bytes: &[u8], description: &str) -> Result<Value, String> {
    vscode::jsonc(&String::from_utf8_lossy(bytes)).ok().filter(Value::is_object).ok_or_else(|| format!("{description} is not valid JSON."))
}

fn short_hash(value: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, value.as_bytes()).as_ref()[..6].iter().map(|b| format!("{b:02x}")).collect()
}

fn theme_id(extension: &str, source: &str) -> String {
    format!("ovx-theme-{}", short_hash(&format!("{extension}:{source}")))
}

fn collection_id(extension: &str) -> String {
    let normalized = format!("open-vsx:{}", extension.to_lowercase());
    let valid = normalized.len() <= 128
        && normalized.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && normalized.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | ':' | '-'));
    if valid { normalized } else { format!("open-vsx:{}", short_hash(extension)) }
}

fn trusted(value: &Value) -> Option<String> {
    let url = s5_net::url::Url::parse(value.as_str()?).ok()?;
    (url.scheme() == "https" && url.host_str().is_some_and(|h| h.eq_ignore_ascii_case("open-vsx.org"))).then(|| url.to_string())
}

fn public_source(value: &Value) -> Option<String> {
    let url = s5_net::url::Url::parse(value.as_str().or(value["url"].as_str())?).ok()?;
    (url.scheme() == "https" && url.username().is_empty() && url.password().is_none()).then(|| url.to_string())
}

fn contributions(manifest: &Value) -> Vec<&Value> {
    manifest["contributes"]["themes"].as_array().map(|themes| themes.iter().filter(|t| t.is_object()).collect()).unwrap_or_default()
}

fn license_matches(manifest: &Value, license: &str) -> bool {
    manifest["license"].as_str().is_some_and(|l| l.trim().eq_ignore_ascii_case(license))
}

/// `extensionFromDetail`: `Ok(None)` for a license the app does not import.
fn extension_from_detail(value: &Value) -> Result<Option<Extension>, ()> {
    let files = &value["files"];
    let field = |key: &str| value[key].as_str().map(str::trim).unwrap_or("").to_owned();
    let (namespace, extension, version, license) = (field("namespace"), field("name"), field("version"), field("license"));
    let (Some(manifest_url), Some(sha256_url), Some(vsix_url)) = (trusted(&files["manifest"]), trusted(&files["sha256"]), trusted(&files["download"])) else { return Err(()) };
    if namespace.is_empty() || extension.is_empty() || version.is_empty() {
        return Err(());
    }
    if !LICENSES.contains(&license.as_str()) {
        return Ok(None);
    }
    let id = format!("{namespace}.{extension}");
    let name = Some(field("displayName")).filter(|n| !n.is_empty()).unwrap_or_else(|| extension.clone());
    Ok(Some(Extension {
        collection_id: collection_id(&id),
        id,
        name,
        description: value["description"].as_str().unwrap_or("").to_owned(),
        downloads: value["downloadCount"].as_f64().filter(|n| n.is_finite()).unwrap_or(0.0),
        source_url: public_source(&value["repository"]).or_else(|| public_source(&value["homepage"])).or_else(|| public_source(&value["url"])),
        manifest_url,
        sha256_url,
        vsix_url,
        namespace,
        extension,
        version,
        license,
    }))
}

/// The detail, then the advertised manifest: only extensions that contribute themes under the
/// license they advertise are listed.
fn detail(namespace: &str, name: &str) -> Result<Option<Extension>, ()> {
    let bytes = get(&format!("{API}/{}/{}", encode(namespace), encode(name)), MAX_DETAIL_BYTES, "", "").map_err(drop)?;
    let Some(extension) = extension_from_detail(&serde_json::from_slice(&bytes).map_err(drop)?)? else { return Ok(None) };
    let manifest = object(&get(&extension.manifest_url, MAX_DETAIL_BYTES, "", "").map_err(drop)?, "Extension manifest").map_err(drop)?;
    Ok((!contributions(&manifest).is_empty() && license_matches(&manifest, &extension.license)).then_some(extension))
}

/// `searchOpenVsxThemes`.
fn search(query: &str, sort: &str) -> Found {
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let url = format!("{API}/-/search?query={}&category=Themes&sortBy={sort}&sortOrder=desc&size=16", encode(query));
    let bytes = get(&url, MAX_SEARCH_BYTES, "Open VSX search is unavailable right now.", "Open VSX returned an unexpectedly large response.")?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "Open VSX returned an unreadable response.")?;
    let Some(candidates) = value["extensions"].as_array() else { return Err("Open VSX returned an unreadable search response.".into()) };
    let identities: Vec<(&str, &str)> = candidates
        .iter()
        .filter_map(|c| Some((c["namespace"].as_str().filter(|n| !n.is_empty())?, c["name"].as_str().filter(|n| !n.is_empty())?)))
        .take(16)
        .collect();
    let details: Vec<Result<Option<Extension>, ()>> = std::thread::scope(|scope| {
        let handles: Vec<_> = identities.iter().map(|&(namespace, name)| scope.spawn(move || detail(namespace, name))).collect();
        handles.into_iter().map(|h| h.join().unwrap_or(Err(()))).collect()
    });
    if !identities.is_empty() && details.iter().all(Result::is_err) {
        return Err("Open VSX theme details are unavailable right now.".into());
    }
    Ok(details.into_iter().filter_map(|d| d.ok().flatten()).take(8).collect())
}

/// `sanitizeThemeObject`: only the workbench colors the conversion reads, and the include.
fn sanitize(value: &Value) -> Value {
    let colors: serde_json::Map<String, Value> = value["colors"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(key, color)| USED_COLORS.contains(&key.as_str()) && color.as_str().is_some_and(|c| c.len() <= MAX_COLOR))
        .map(|(key, color)| (key.clone(), color.clone()))
        .collect();
    let mut out = json!({"colors": colors});
    if let Some(include) = value["include"].as_str() {
        out["include"] = json!(include);
    }
    out
}

/// `normalizePackagePath`: a path inside the package's `extension/` folder.
fn normalize(path: &str, relative_to: &str) -> Result<String, String> {
    let drive = path.as_bytes().get(1) == Some(&b':') && path.as_bytes()[0].is_ascii_alphabetic();
    if path.len() > MAX_PATH || path.contains('\0') || path.starts_with('/') || drive {
        return Err("Theme path is not a safe relative package path.".into());
    }
    let mut segments: Vec<&str> = relative_to.split('/').collect();
    segments.pop();
    let input = path.replace('\\', "/");
    for segment in input.split('/') {
        match segment {
            "" | "." => {}
            ".." if segments.len() <= 1 => return Err("Theme path escapes the extension package.".into()),
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    if segments.first() != Some(&"extension") {
        segments.insert(0, "extension");
    }
    Ok(segments.join("/"))
}

fn contribution_type(ui_theme: &Value) -> Option<&'static str> {
    match ui_theme.as_str()? {
        "vs" => Some("light"),
        "vs-dark" => Some("dark"),
        "hc-black" => Some("hc-black"),
        "hc-light" => Some("hc-light"),
        _ => None,
    }
}

type Reader<'a> = &'a dyn Fn(&str, &str) -> Result<Value, String>;

/// `loadThemeObject`: a theme file with its `include` chain folded underneath it.
fn load(read: Reader, path: &str, cache: &mut HashMap<String, Value>, budget: &mut usize, ancestors: &[String]) -> Result<Value, String> {
    if ancestors.len() >= MAX_INCLUDE_DEPTH {
        return Err("Theme includes are nested too deeply.".into());
    }
    if ancestors.iter().any(|a| a == path) {
        return Err("Theme includes contain a cycle.".into());
    }
    if let Some(cached) = cache.get(path) {
        return Ok(cached.clone());
    }
    *budget += 1;
    if *budget > MAX_THEME_FILES {
        return Err("That extension references too many theme files.".into());
    }
    let mut value = sanitize(&read(path, path)?);
    if let Some(include) = value["include"].as_str().map(str::to_owned) {
        let include = normalize(&include, path)?;
        let mut chain = ancestors.to_vec();
        chain.push(path.to_owned());
        let base = load(read, &include, cache, budget, &chain)?;
        let mut colors = base["colors"].as_object().cloned().unwrap_or_default();
        colors.extend(value["colors"].as_object().cloned().unwrap_or_default());
        value["colors"] = Value::Object(colors);
    }
    cache.insert(path.to_owned(), value.clone());
    Ok(value)
}

/// `inspectZipDirectory`: the end record and central directory, checked before anything is
/// unpacked, with the archive comment stripped.
fn inspect_directory(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let invalid = || "That extension package has an invalid ZIP directory.".to_owned();
    let u16_at = |at: usize| bytes.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize);
    let u32_at = |at: usize| bytes.get(at..at + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    let minimum = bytes.len().saturating_sub(65_557);
    let end = (minimum..=bytes.len().saturating_sub(22))
        .rev()
        .find(|&at| bytes.len() >= 22 && u32_at(at) == Some(0x0605_4b50) && u16_at(at + 20).is_some_and(|comment| at + 22 + comment == bytes.len()))
        .ok_or("That extension package has no ZIP directory.")?;
    let size = u32_at(end + 12).ok_or_else(invalid)? as usize;
    let start = u32_at(end + 16).ok_or_else(invalid)? as usize;
    if start.checked_add(size) != Some(end) {
        return Err(invalid());
    }
    let (mut offset, mut entries, mut total) = (start, 0, 0u64);
    while offset < end {
        if offset + 46 > end || u32_at(offset) != Some(0x0201_4b50) {
            return Err(invalid());
        }
        entries += 1;
        if entries > MAX_ZIP_ENTRIES {
            return Err("That extension package has too many files.".into());
        }
        let (compressed, uncompressed) = (u32_at(offset + 20).ok_or_else(invalid)?, u32_at(offset + 24).ok_or_else(invalid)?);
        if compressed == u32::MAX || uncompressed == u32::MAX {
            return Err("That extension package has unsupported ZIP64 metadata.".into());
        }
        total += u64::from(uncompressed);
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err("That extension package expands beyond the safe import limit.".into());
        }
        if uncompressed > 0 && (compressed == 0 || u64::from(uncompressed) > u64::from(compressed) * MAX_COMPRESSION_RATIO) {
            return Err("That extension package has an unsafe compression ratio.".into());
        }
        offset += 46 + u16_at(offset + 28).ok_or_else(invalid)? + u16_at(offset + 30).ok_or_else(invalid)? + u16_at(offset + 32).ok_or_else(invalid)?;
    }
    if offset != end {
        return Err(invalid());
    }
    let mut out = bytes[..end + 22].to_vec();
    out[end + 20] = 0;
    out[end + 21] = 0;
    Ok(out)
}

/// The package's SHA-256 against Open VSX's `sha256` file (its first word).
fn checksum_matches(package: &[u8], published: &[u8]) -> Result<(), String> {
    let text = String::from_utf8_lossy(published);
    let expected = text.split_whitespace().next().filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit())).ok_or("That Open VSX theme has an invalid checksum.")?;
    let actual: String = ring::digest::digest(&ring::digest::SHA256, package).as_ref().iter().map(|b| format!("{b:02x}")).collect();
    if actual.eq_ignore_ascii_case(expected) { Ok(()) } else { Err("That Open VSX theme failed its integrity check.".into()) }
}

type Archive = zip::ZipArchive<std::io::Cursor<Vec<u8>>>;

/// `inspectZipDirectory`, then `JSZip.loadAsync` and `inspectZip`: no entry may name a path
/// outside the package.
fn open_package(bytes: &[u8]) -> Result<Archive, String> {
    let inspected = inspect_directory(bytes)?;
    let archive = zip::ZipArchive::new(std::io::Cursor::new(inspected)).map_err(|_| "That Open VSX extension package could not be opened.")?;
    if archive.len() > MAX_ZIP_ENTRIES {
        return Err("That extension package has too many files.".into());
    }
    for name in archive.file_names() {
        normalize(name, "extension/")?;
    }
    Ok(archive)
}

/// `readZipText`: one file, refused above the theme size limit before and while reading.
fn read_entry(archive: &mut Archive, path: &str, description: &str) -> Result<Vec<u8>, String> {
    let file = archive.by_name(path).map_err(|_| format!("{description} is missing from the extension package."))?;
    if file.size() > MAX_THEME_BYTES {
        return Err(format!("{description} is too large."));
    }
    let mut bytes = Vec::new();
    file.take(MAX_THEME_BYTES + 1).read_to_end(&mut bytes).map_err(|_| "That Open VSX extension package could not be opened.".to_owned())?;
    if bytes.len() as u64 > MAX_THEME_BYTES {
        return Err(format!("{description} is too large."));
    }
    Ok(bytes)
}

/// `importOpenVsxThemeExtension`: the advertised manifest, the checksummed VSIX, then the
/// package's own manifest must still match the listing, and every contributed theme convert.
fn install(extension: &Extension) -> Result<Vec<Value>, String> {
    let advertised = object(&get(&extension.manifest_url, MAX_DETAIL_BYTES, "That Open VSX extension has no readable manifest.", "That Open VSX extension manifest is too large.")?, "Extension manifest")?;
    match contributions(&advertised).len() {
        0 => return Err("That extension does not contain color themes.".into()),
        n if n > MAX_THEMES => return Err("That extension contains too many color themes to import safely.".into()),
        _ => {}
    }
    let package = get(&extension.vsix_url, MAX_VSIX_BYTES, "That Open VSX theme could not be downloaded.", "That theme extension is too large to import safely.")?;
    let published = get(&extension.sha256_url, 256, "That Open VSX theme has no readable checksum.", "That Open VSX checksum response is invalid.")?;
    checksum_matches(&package, &published)?;
    let archive = RefCell::new(open_package(&package)?);
    let fetch = |path: &str, description: &str| -> Result<Value, String> { object(&read_entry(&mut archive.borrow_mut(), path, description)?, description) };
    let manifest = fetch("extension/package.json", "Extension manifest")?;
    let (publisher, name) = (manifest["publisher"].as_str().unwrap_or(""), manifest["name"].as_str().unwrap_or(""));
    if !publisher.eq_ignore_ascii_case(&extension.namespace)
        || !format!("{publisher}.{name}").eq_ignore_ascii_case(&extension.id)
        || manifest["version"].as_str() != Some(extension.version.as_str())
    {
        return Err("That extension package does not match the selected Open VSX theme.".into());
    }
    if !license_matches(&manifest, &extension.license) {
        return Err("That extension package does not match its advertised license.".into());
    }
    let contributed = contributions(&manifest);
    if contributed.is_empty() {
        return Err("That extension does not contain color themes.".into());
    }
    if contributed.len() > MAX_THEMES {
        return Err("That extension contains too many color themes to import safely.".into());
    }
    let stock = |dark: bool| super::palette(&super::builtins()[0], dark).clone();
    let (mut cache, mut budget) = (HashMap::new(), 0);
    let mut parsed: Vec<(Value, String)> = Vec::new();
    for contribution in contributed {
        let mut convert = || -> Result<(Value, String), String> {
            let path = normalize(contribution["path"].as_str().ok_or("theme path is missing")?, "extension/")?;
            let mut theme = load(&fetch, &path, &mut cache, &mut budget, &[])?;
            let label = contribution["label"].as_str().map(str::trim).filter(|l| !l.is_empty()).unwrap_or(&extension.name);
            theme["displayName"] = json!(label);
            if let Some(kind) = contribution_type(&contribution["uiTheme"]) {
                theme["type"] = json!(kind);
            }
            if !vscode::is_theme(&theme) {
                return Err("not a VS Code color theme".into());
            }
            Ok((vscode::convert(&theme, stock)?, path))
        };
        parsed.push(convert().map_err(|_| "One or more color themes in that extension could not be imported safely.")?);
    }
    Ok(assemble(extension, parsed))
}

/// Stable ids per source file, families paired, and the whole set tagged as one collection.
fn assemble(extension: &Extension, parsed: Vec<(Value, String)>) -> Vec<Value> {
    let owner = extension.id.to_lowercase();
    let mut occurrences: HashMap<&str, usize> = HashMap::new();
    let sources: Vec<String> = parsed
        .iter()
        .map(|(_, path)| {
            let total = parsed.iter().filter(|(_, p)| p == path).count();
            let seen = occurrences.entry(path.as_str()).or_insert(0);
            *seen += 1;
            if total == 1 || *seen == 1 { path.clone() } else { format!("{path}\0{}", *seen - 1) }
        })
        .collect();
    let named = vscode::resolve_collisions(parsed.iter().map(|(theme, path)| (theme.clone(), path.rsplit('/').next().map(str::to_owned))).collect());
    let identified: Vec<Value> = named
        .into_iter()
        .zip(&sources)
        .map(|(mut theme, source)| {
            theme["id"] = json!(theme_id(&owner, source));
            theme
        })
        .collect();
    let paired_id = |light: &Value, dark: &Value| {
        let mut ids = [light["id"].as_str().unwrap_or(""), dark["id"].as_str().unwrap_or("")];
        ids.sort_unstable();
        theme_id(&owner, &ids.join(":"))
    };
    let paired = vscode::pair(identified, Some(&paired_id as &dyn Fn(&Value, &Value) -> String));
    let label: String = extension.name.chars().take(48).collect();
    vscode::resolve_collisions(paired.into_iter().map(|t| (t, None)).collect())
        .into_iter()
        .map(|mut theme| {
            theme["collection"] = json!({"id": extension.collection_id, "label": label});
            theme
        })
        .collect()
}

fn installed(library: &[Value], collection: &str) -> Vec<Value> {
    library.iter().filter(|t| t["collection"]["id"] == collection).cloned().collect()
}

/// `replaceCustomThemeCollection`: the collection's themes swap in where the old ones stood.
fn replace_collection(library: &[Value], collection: &str, themes: &[Value], expected: &[Value]) -> Result<Vec<Value>, String> {
    if themes.is_empty() {
        return Err("A theme collection cannot be empty.".into());
    }
    let ids: HashSet<&str> = themes.iter().filter_map(|t| t["id"].as_str()).collect();
    if ids.len() != themes.len() || themes.iter().any(|t| t["collection"]["id"] != collection) {
        return Err("That theme collection is invalid.".into());
    }
    if installed(library, collection) != expected {
        return Err("Your installed themes changed while this package was downloading. Try again.".into());
    }
    let occupied: HashSet<&str> = super::builtins().iter().chain(library.iter().filter(|t| t["collection"]["id"] != collection)).filter_map(|t| t["id"].as_str()).collect();
    if let Some(taken) = themes.iter().map(|t| (t, t["id"].as_str().unwrap_or(""))).find(|(_, id)| super::RESERVED.contains(id) || occupied.contains(id)).map(|(t, _)| t) {
        return Err(format!("A theme named \"{}\" is already installed.", taken["label"].as_str().unwrap_or("")));
    }
    let mut next = Vec::new();
    let mut inserted = false;
    for theme in library {
        if theme["collection"]["id"] != collection {
            next.push(theme.clone());
        } else if !inserted {
            next.extend(themes.iter().cloned());
            inserted = true;
        }
    }
    if !inserted {
        next.extend(themes.iter().cloned());
    }
    Ok(next)
}

/// `Intl.NumberFormat` compact with one fraction digit: 472.2K, 1.5M.
fn downloads(n: f64) -> String {
    let (value, suffix) = match n {
        n if n >= 1e9 => (n / 1e9, "B"),
        n if n >= 1e6 => (n / 1e6, "M"),
        n if n >= 1e3 => (n / 1e3, "K"),
        n => (n, ""),
    };
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 { format!("{rounded:.0}{suffix}") } else { format!("{rounded:.1}{suffix}") }
}

fn search_state(s: &mut App) -> &mut Search {
    &mut s.ui.settings.theme_search
}

fn sort_of(search: &Search) -> &'static str {
    search.sort.unwrap_or(SORTS[0].0)
}

fn search_now(s: &mut App) {
    let (query, sort) = (s.ui.settings.theme_search.query.text.clone(), sort_of(&s.ui.settings.theme_search));
    let ui = search_state(s);
    ui.due = None;
    ui.error = None;
    if query.trim().is_empty() {
        ui.searching = None;
        ui.results = None;
        return;
    }
    let slot = spawn(s, move || search(&query, sort));
    search_state(s).searching = Some(slot);
}

/// Web debounces typing by 350ms: a timer thread wakes the poll, which searches once the latest
/// keystroke's deadline has passed.
fn queue(s: &mut App) {
    s.ui.settings.theme_search.due = Some(Instant::now() + DEBOUNCE);
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        std::thread::sleep(DEBOUNCE);
        if let Some(waker) = waker {
            waker();
        }
    });
}

fn set_query(s: &mut App, query: &str) {
    s.ui.settings.theme_search.query = TextEdit::new(query);
    search_now(s);
}

fn begin_install(s: &mut App, extension: Extension, confirmed: bool) {
    let expected = installed(&super::custom_themes(s), &extension.collection_id);
    let ui = search_state(s);
    ui.error = None;
    if !expected.is_empty() && !confirmed {
        ui.confirm = Some(extension);
        return;
    }
    let target = extension.clone();
    let slot = spawn(s, move || install(&target));
    search_state(s).installing = Some((extension, expected, slot));
}

fn installed_themes(s: &mut App, extension: &Extension, expected: &[Value], result: Result<Vec<Value>, String>) {
    let library = super::custom_themes(s);
    let applied = result.and_then(|themes| replace_collection(&library, &extension.collection_id, &themes, expected).map(|next| (next, themes)));
    match applied {
        Ok((next, themes)) => {
            super::super::update(s, "customThemes", json!(next));
            let verb = if expected.is_empty() { "added" } else { "updated" };
            let title = match themes.as_slice() {
                [theme] => format!("{} {verb}", theme["label"].as_str().unwrap_or("Theme")),
                _ => format!("{} themes {verb}", themes.len()),
            };
            let labels = themes.iter().filter_map(|t| t["label"].as_str()).collect::<Vec<_>>().join(", ");
            crate::shell::palette::toast(s, &title, Some(&labels));
            super::close_import(s);
        }
        Err(error) => search_state(s).error = Some(error),
    }
}

/// Picks up finished searches and installs, and a debounced search that came due.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let ui = &mut s.ui.settings.theme_search;
    if let Some(found) = ui.searching.as_ref().and_then(|slot| lock(slot).take()) {
        ui.searching = None;
        // A result for a query typed over since is dropped; the newer one searches next.
        if ui.due.is_none() {
            match found {
                Ok(results) => ui.results = Some(results),
                Err(error) => ui.error = Some(error),
            }
        }
        changed = true;
    }
    let due = ui.due.is_some_and(|deadline| Instant::now() >= deadline);
    if due && ui.searching.is_none() {
        changed = true;
        search_now(s);
    }
    let ui = &mut s.ui.settings.theme_search;
    if let Some(result) = ui.installing.as_ref().and_then(|(.., slot)| lock(slot).take())
        && let Some((extension, expected, _)) = ui.installing.take()
    {
        installed_themes(s, &extension, &expected, result);
        changed = true;
    }
    changed
}

/// The `ThemeSearchSection` at the top of the add-theme dialog; the sort list goes to `menus`.
pub fn section(s: &App, t: &T, menus: &mut Vec<El>) -> El {
    let ui = &s.ui.settings.theme_search;
    let busy = ui.installing.is_some();
    let searching = ui.searching.is_some() || ui.due.is_some();
    let lead = if searching {
        kit::Spinner::new().size("xs").tone("muted").build(&kit::theme(t))
    } else {
        rune::icon(&icons::SEARCH, 14.0).color(t.c.muted_foreground)
    };
    let field = Input::new(&ui.query)
        .placeholder("Search themes...")
        .size("lg")
        .on_edit(|s: &mut App| &mut s.ui.settings.theme_search.query)
        .on_change(|s: &mut App, _| queue(s))
        .on_submit(|s: &mut App, _| {
            if s.ui.settings.theme_search.searching.is_none() && s.ui.settings.theme_search.installing.is_none() {
                search_now(s);
            }
        })
        .build(t)
        .key("theme-search")
        .single_line()
        .label("Search Open VSX themes")
        .pl(34.0);
    let mut out = col()
        .w_full()
        .gap(12.0)
        .child(
            col()
                .gap(2.0)
                .child(text("Search community themes").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text("Find open-source themes from Open VSX.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        )
        .child(el().w_full().child(field).child(row().pin(Some(12.0), Some(0.0), None, Some(0.0)).center_y().child(lead)));
    if !searching || ui.results.is_some() {
        let mut popular = row().grow(1.0).min_w(0.0).gap(6.0).center_y().child(text("Popular").font(theme::TEXT_XS).color(t.c.muted_foreground));
        for suggestion in SUGGESTIONS {
            popular = popular.child(Button::new(suggestion).size("xs").variant("ghost").disabled(busy).on_press(move |s: &mut App| set_query(s, suggestion)).build(t));
        }
        let mut bar = row().w_full().gap(8.0).center_y().child(popular);
        if ui.results.as_ref().is_some_and(|r| !r.is_empty()) {
            let mut select = kit::Select::new("theme-search-sort", &SORTS.map(|(value, _)| value))
                .label("Sort themes")
                .width(160.0)
                .size("sm")
                .current(Some(sort_of(ui)))
                .disabled(busy)
                .align_end()
                .open(ui.sort_open && !busy)
                .on_toggle(|s: &mut App| s.ui.settings.theme_search.sort_open ^= true)
                .on_close(|s: &mut App| s.ui.settings.theme_search.sort_open = false)
                .on_choose(|s: &mut App, value: &str, _: &mut rune::Tx| {
                    let ui = &mut s.ui.settings.theme_search;
                    ui.sort_open = false;
                    ui.sort = SORTS.iter().map(|(v, _)| *v).find(|v| *v == value);
                    search_now(s);
                });
            for (value, label) in SORTS {
                select = select.option_name(value, label);
            }
            bar = bar.child(row().gap(8.0).center_y().child(text("Sort").font(theme::TEXT_XS).color(t.c.muted_foreground)).child(select.build(&kit::theme(t), menus)));
        }
        out = out.child(bar);
    }
    if let Some(error) = &ui.error {
        out = out.child(
            el().w_full().padding(12.0).radius(8.0).border(1.0, t.c.destructive.alpha_mul(0.32)).fill(t.c.destructive.alpha_mul(0.04)).role("status").child(
                text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive).wrap(),
            ),
        );
    }
    match &ui.results {
        None if searching => {
            out = out.child(
                row()
                    .w_full()
                    .min_h(80.0)
                    .gap(8.0)
                    .center()
                    .child(kit::Spinner::new().size("sm").tone("muted").build(&kit::theme(t)))
                    .child(text("Searching themes...").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            );
        }
        None => {}
        Some(results) if results.is_empty() => {
            out = out.child(
                col()
                    .w_full()
                    .min_h(160.0)
                    .gap(4.0)
                    .center()
                    .radius(16.0)
                    .border(1.0, t.c.border)
                    .border_dash(4.0, 3.0)
                    .child(text("No supported open-source themes found").font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(text("Try a broader search.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
            );
        }
        Some(results) => {
            let library = super::custom_themes(s);
            let mut grid = col().w_full().gap(8.0);
            for pair in results.chunks(2) {
                let mut line = row().w_full().gap(8.0).items_start();
                for extension in pair {
                    line = line.child(card(extension, &library, ui, t));
                }
                if pair.len() == 1 {
                    line = line.child(el().grow(1.0).min_w(0.0));
                }
                grid = grid.child(line);
            }
            out = out.child(grid);
        }
    }
    out
}

fn card(extension: &Extension, library: &[Value], ui: &Search, t: &T) -> El {
    let is_installed = library.iter().any(|theme| theme["collection"]["id"] == extension.collection_id.as_str());
    let installing = ui.installing.as_ref().is_some_and(|(e, ..)| e.id == extension.id);
    let (action, progress) = if is_installed { ("Update", "Updating") } else { ("Install", "Installing") };
    let initial = extension.name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    let mut footer = row().w_full().gap(8.0).center_y();
    footer = match extension.source_url.clone() {
        Some(url) => footer.child(
            Button::new(format!("View source for {}", extension.name))
                .icon(&icons::EXTERNAL_LINK)
                .size("icon-micro")
                .variant("ghost-muted")
                .on_press(move |s: &mut App| crate::settings::auth::open_url(s, &url))
                .build(t),
        ),
        None => footer,
    };
    let target = extension.clone();
    let mut button = Button::new(if installing { format!("{progress}...") } else { action.to_owned() })
        .size("xs")
        .variant("outline")
        .disabled(ui.installing.is_some())
        .on_press(move |s: &mut App| begin_install(s, target.clone(), false));
    if !installing {
        button = button.icon(if is_installed { &icons::REFRESH_CW } else { &icons::PACKAGE_PLUS });
    }
    footer = footer.child(el().grow(1.0)).child(button.build(t).label(format!("{} {}", if installing { progress } else { action }, extension.name)));
    let description = if extension.description.trim().is_empty() { "A community color theme for your editor.".to_owned() } else { extension.description.clone() };
    col()
        .key(format!("ovx-{}", extension.id))
        .grow(1.0)
        .min_w(0.0)
        .gap(12.0)
        .padding(12.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.7))
        .fill(t.c.card.alpha_mul(0.6))
        .hover(t.c.accent.alpha_mul(0.2))
        .child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .child(
                    el().size(40.0, 40.0).radius(8.0).fill(t.c.muted).center().child(text(initial).font(theme::SM_MEDIUM).color(t.c.muted_foreground)),
                )
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(extension.name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                        .child(
                            text(format!("{} · {} downloads", extension.namespace, downloads(extension.downloads)))
                                .font(theme::TEXT_XS)
                                .color(t.c.muted_foreground)
                                .truncate(),
                        ),
                ),
        )
        .child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().max_lines(2).min_h(32.0))
        .child(footer)
}

/// The "Update “…”?" confirmation for a collection that is already installed.
pub fn confirm_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(extension) = &s.ui.settings.theme_search.confirm else { return };
    let target = extension.clone();
    let cancel = |s: &mut App| s.ui.settings.theme_search.confirm = None;
    kit::Dialog::new("theme-update", format!("Update \u{201c}{}\u{201d}?", extension.name))
        .alert()
        .open(true)
        .description("This replaces its installed variants, including any local edits. Variants no longer in the extension will be removed.")
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(
                    Button::new("Update theme")
                        .on_press(move |s: &mut App| {
                            s.ui.settings.theme_search.confirm = None;
                            begin_install(s, target.clone(), true);
                        })
                        .build(t),
                ),
        )
        .on_close(cancel)
        .build(t, layers);
}

/// Escape closes the sort list or the update confirmation before the dialog.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.theme_search;
    std::mem::take(&mut ui.sort_open) || ui.confirm.take().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(id: &str) -> Extension {
        Extension {
            id: id.into(),
            collection_id: collection_id(id),
            namespace: id.split('.').next().unwrap_or("").into(),
            extension: id.split('.').nth(1).unwrap_or("").into(),
            name: "Solar".into(),
            description: String::new(),
            downloads: 0.0,
            source_url: None,
            manifest_url: String::new(),
            sha256_url: String::new(),
            vsix_url: String::new(),
            version: "1.0.0".into(),
            license: "MIT".into(),
        }
    }

    #[test]
    fn listings_keep_only_trusted_urls_and_supported_licenses() {
        let detail = |license: &str, download: &str| {
            json!({"namespace": "acme", "name": "solar", "displayName": " ", "version": "1.0.0", "license": license, "repository": "https://user:pw@github.com/acme/solar", "homepage": {"url": "https://acme.dev"},
                "files": {"manifest": "https://open-vsx.org/api/acme/solar/1.0.0/file/package.json", "sha256": "https://open-vsx.org/api/acme/solar/1.0.0/file/acme.solar-1.0.0.sha256", "download": download}})
        };
        let extension = extension_from_detail(&detail("MIT", "https://open-vsx.org/x.vsix")).unwrap().unwrap();
        assert_eq!((extension.id.as_str(), extension.name.as_str(), extension.collection_id.as_str()), ("acme.solar", "solar", "open-vsx:acme.solar"));
        assert_eq!(extension.source_url.as_deref(), Some("https://acme.dev/"), "credentialed source links are dropped");
        assert_eq!(extension_from_detail(&detail("GPL-3.0", "https://open-vsx.org/x.vsix")), Ok(None));
        assert_eq!(extension_from_detail(&detail("MIT", "https://evil.example/x.vsix")), Err(()));
        assert!(collection_id("Acme.Ünicode").starts_with("open-vsx:") && collection_id("Acme.Ünicode").len() == "open-vsx:".len() + 12);
        assert_eq!(downloads(472_190.0), "472.2K");
        assert_eq!(downloads(2_000_000.0), "2M");
        assert_eq!(downloads(12.0), "12");
    }

    /// A stored (uncompressed) ZIP with one entry and an archive comment.
    fn stored_zip(name: &str, data: &[u8], comment: &[u8]) -> Vec<u8> {
        let crc = data.iter().fold(!0u32, |mut crc, &byte| {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
            crc
        }) ^ !0;
        let (n, len) = (name.len() as u16, data.len() as u32);
        let mut out = Vec::new();
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [crc, len, len] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(data);
        let directory = out.len() as u32;
        out.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 20, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        for v in [crc, len, len] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&n.to_le_bytes());
        out.extend_from_slice(&[0; 12]);
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        let size = out.len() as u32 - directory;
        out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0, 1, 0, 1, 0]);
        out.extend_from_slice(&size.to_le_bytes());
        out.extend_from_slice(&directory.to_le_bytes());
        out.extend_from_slice(&(comment.len() as u16).to_le_bytes());
        out.extend_from_slice(comment);
        out
    }

    #[test]
    fn packages_pass_the_checksum_and_zip_limits_web_checks() {
        let comment = b"PK\x05\x06 lookalike";
        let package = stored_zip("extension/package.json", br#"{"name":"solar"}"#, comment);
        let hash: String = ring::digest::digest(&ring::digest::SHA256, &package).as_ref().iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(checksum_matches(&package, format!("{} acme.solar-1.0.0.vsix\n", hash.to_uppercase()).as_bytes()), Ok(()));
        assert_eq!(checksum_matches(b"other", hash.as_bytes()).unwrap_err(), "That Open VSX theme failed its integrity check.");
        assert_eq!(checksum_matches(&package, b"not-a-hash").unwrap_err(), "That Open VSX theme has an invalid checksum.");

        let inspected = inspect_directory(&package).unwrap();
        assert_eq!((inspected.len(), &inspected[inspected.len() - 2..]), (package.len() - comment.len(), &[0u8, 0][..]), "the comment is stripped");
        let mut archive = open_package(&package).unwrap();
        assert_eq!(read_entry(&mut archive, "extension/package.json", "Extension manifest").unwrap(), br#"{"name":"solar"}"#);
        assert_eq!(read_entry(&mut archive, "extension/missing.json", "Theme").unwrap_err(), "Theme is missing from the extension package.");

        let directory = package.len() - comment.len() - 22 - "extension/package.json".len() - 46;
        let patched = |at: usize, value: u32| {
            let mut bytes = package.clone();
            bytes[directory + at..directory + at + 4].copy_from_slice(&value.to_le_bytes());
            inspect_directory(&bytes).unwrap_err()
        };
        assert_eq!(patched(24, 16 * 201), "That extension package has an unsafe compression ratio.");
        assert_eq!(patched(24, u32::MAX), "That extension package has unsupported ZIP64 metadata.");
        assert_eq!(patched(0, 0), "That extension package has an invalid ZIP directory.");
        assert_eq!(inspect_directory(&package[..package.len() - 1]).unwrap_err(), "That extension package has no ZIP directory.");
        assert!(open_package(&stored_zip("../escape.json", b"{}", b"")).is_err(), "no entry may leave the package");
    }

    #[test]
    fn package_paths_stay_inside_the_extension() {
        assert_eq!(normalize("./themes/dark.json", "extension/").unwrap(), "extension/themes/dark.json");
        assert_eq!(normalize("../base.json", "extension/themes/dark.json").unwrap(), "extension/base.json");
        assert_eq!(normalize("themes\\light.json", "extension/").unwrap(), "extension/themes/light.json");
        for bad in ["/etc/passwd", "C:/x.json", "../../x.json", "a\0b"] {
            assert!(normalize(bad, "extension/").is_err(), "FAILED: {bad}");
        }
    }

    #[test]
    fn includes_fold_under_the_theme_and_cycles_fail() {
        let files = HashMap::from([
            ("extension/dark.json", json!({"include": "./base.json", "colors": {"editor.background": "#111111", "tokenColors.x": "#000"}})),
            ("extension/base.json", json!({"colors": {"editor.background": "#222222", "editor.foreground": "#eeeeee"}})),
            ("extension/loop.json", json!({"include": "./loop.json", "colors": {}})),
        ]);
        let read = |path: &str, _: &str| files.get(path).cloned().ok_or_else(|| "missing".to_owned());
        let (mut cache, mut budget) = (HashMap::new(), 0);
        let theme = load(&read, "extension/dark.json", &mut cache, &mut budget, &[]).unwrap();
        assert_eq!(theme["colors"], json!({"editor.background": "#111111", "editor.foreground": "#eeeeee"}), "unused keys go, the theme wins");
        assert!(load(&read, "extension/loop.json", &mut cache, &mut budget, &[]).unwrap_err().contains("cycle"));
    }

    #[test]
    fn a_package_becomes_one_collection_with_stable_paired_ids() {
        let theme = |label: &str, dark: bool| {
            json!({"id": vscode::theme_id(label), "label": label, "appearance": if dark { "dark" } else { "light" }, "colors": {"canvas": if dark { "#000000" } else { "#ffffff" }}})
        };
        let extension = listed("acme.solar");
        let themes = assemble(&extension, vec![(theme("Solar Light", false), "extension/light.json".into()), (theme("Solar Dark", true), "extension/dark.json".into())]);
        assert_eq!(themes.len(), 1, "a light and dark family pair");
        let ids = [theme_id("acme.solar", "extension/light.json"), theme_id("acme.solar", "extension/dark.json")];
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(themes[0]["id"], theme_id("acme.solar", &sorted.join(":")));
        assert!(themes[0]["id"].as_str().unwrap().starts_with("ovx-theme-") && themes[0]["id"].as_str().unwrap().len() == "ovx-theme-".len() + 12);
        assert_eq!(themes[0]["collection"], json!({"id": "open-vsx:acme.solar", "label": "Solar"}));
    }

    #[test]
    fn a_collection_replaces_in_place_and_refuses_conflicts() {
        let tagged = |id: &str, collection: &str| json!({"id": id, "label": id, "collection": {"id": collection}});
        let mine = json!({"id": "custom-1", "label": "Mine"});
        let library = vec![mine.clone(), tagged("ovx-a", "open-vsx:x"), tagged("ovx-b", "open-vsx:x"), json!({"id": "custom-2"})];
        let expected = installed(&library, "open-vsx:x");
        let next = replace_collection(&library, "open-vsx:x", &[tagged("ovx-c", "open-vsx:x")], &expected).unwrap();
        assert_eq!(next.iter().map(|t| t["id"].as_str().unwrap()).collect::<Vec<_>>(), ["custom-1", "ovx-c", "custom-2"]);
        let fresh = replace_collection(&library, "open-vsx:y", &[tagged("ovx-y", "open-vsx:y")], &[]).unwrap();
        assert_eq!(fresh.last().unwrap()["id"], "ovx-y", "a new collection appends");
        assert!(replace_collection(&library, "open-vsx:x", &[tagged("ovx-c", "open-vsx:x")], &[]).unwrap_err().contains("changed"));
        assert_eq!(replace_collection(&library, "open-vsx:y", &[tagged("custom-1", "open-vsx:y")], &[]).unwrap_err(), "A theme named \"custom-1\" is already installed.");
        assert!(replace_collection(&library, "open-vsx:y", &[tagged("grove", "open-vsx:y")], &[]).is_err(), "builtin ids are taken");
        assert!(replace_collection(&library, "open-vsx:y", &[tagged("a", "open-vsx:z")], &[]).unwrap_err().contains("invalid"));
        assert!(replace_collection(&library, "open-vsx:y", &[], &[]).is_err());
    }
}
