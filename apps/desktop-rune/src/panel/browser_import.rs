//! Browser import: reads a browser's cookies on this machine and writes them into Rune's Chrome
//! over CDP, as the Electron app's `BrowserImport` does into its session.
//!
//! Cookie databases are read into memory (never opened for writing) and parsed here, since Rune
//! links no SQLite; a Firefox database's write-ahead log is applied over it. Chromium values use
//! OSCrypt's AES-128-CBC: macOS keys come from the browser's Keychain item (`security`), Linux
//! keys from the fixed "peanuts" passphrase (v10) or libsecret (`secret-tool`, v11). Safari's
//! `Cookies.binarycookies` sits behind Full Disk Access.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use rune::Waker;
use serde_json::{Value, json};

use super::browser::Cmd;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    Chromium,
    Firefox,
    Safari,
}

pub struct Source {
    pub name: &'static str,
    pub engine: Engine,
    service: &'static str,
    account: &'static str,
    /// Under `~`; empty where the browser has no macOS build.
    mac: &'static [&'static str],
    /// Candidate roots under `~`, the first that exists winning; empty without a Linux build.
    linux: &'static [&'static [&'static str]],
    /// The libsecret `application` attribute holding the v11 key.
    secret: &'static str,
}

const APP_SUPPORT: [&str; 2] = ["Library", "Application Support"];

macro_rules! chromium {
    ($name:literal, $service:literal, $account:literal, [$($mac:literal),*], [$($linux:literal),*], $secret:literal) => {
        Source { name: $name, engine: Engine::Chromium, service: $service, account: $account, mac: &[APP_SUPPORT[0], APP_SUPPORT[1], $($mac),*], linux: chromium!(@linux $($linux),*), secret: $secret }
    };
    (@linux) => { &[] };
    (@linux $($linux:literal),+) => { &[&[".config", $($linux),+]] };
}

macro_rules! gecko {
    ($name:literal, $mac:literal, [$([$($linux:literal),+]),+]) => {
        Source { name: $name, engine: Engine::Firefox, service: "", account: "", mac: &[APP_SUPPORT[0], APP_SUPPORT[1], $mac], linux: &[$(&[$($linux),+]),+], secret: "" }
    };
}

/// `BROWSER_IMPORT_SOURCES`, in menu order.
pub const SOURCES: &[Source] = &[
    chromium!("Chrome", "Chrome Safe Storage", "Chrome", ["Google", "Chrome"], ["google-chrome"], "chrome"),
    chromium!("Microsoft Edge", "Microsoft Edge Safe Storage", "Microsoft Edge", ["Microsoft Edge"], ["microsoft-edge"], "msedge"),
    chromium!("Brave", "Brave Safe Storage", "Brave", ["BraveSoftware", "Brave-Browser"], ["BraveSoftware", "Brave-Browser"], "brave"),
    // Vivaldi keeps its Linux key under Chrome's application name, Helium under Chromium's.
    chromium!("Vivaldi", "Vivaldi Safe Storage", "Vivaldi", ["Vivaldi"], ["vivaldi"], "chrome"),
    chromium!("Opera", "Opera Safe Storage", "Opera", ["com.operasoftware.Opera"], ["opera"], "opera"),
    chromium!("Arc", "Arc Safe Storage", "Arc", ["Arc", "User Data"], [], ""),
    chromium!("Helium", "Helium Storage Key", "Helium", ["net.imput.helium"], ["net.imput.helium"], "chromium"),
    Source {
        name: "Safari",
        engine: Engine::Safari,
        service: "",
        account: "",
        mac: &["Library", "Containers", "com.apple.Safari", "Data", "Library", "Cookies"],
        linux: &[],
        secret: "",
    },
    gecko!("Firefox", "Firefox", [[".mozilla", "firefox"], ["snap", "firefox", "common", ".mozilla", "firefox"]]),
    gecko!("Zen", "zen", [[".config", "zen"], [".zen"], [".var", "app", "app.zen_browser.zen", "zen"]]),
];

/// `BROWSER_IMPORT_FAILURE_COPY` for the reasons a Chromium import can fail with.
pub fn failure_copy(reason: &str) -> &'static str {
    match reason {
        "notInstalled" => "Not installed on this machine.",
        "needsFullDiskAccess" => "Give S5 Code Full Disk Access in System Settings → Privacy & Security, then retry.",
        "needsKeychainApproval" => "Needs Keychain access to read its cookies.",
        "keychainItemMissing" => "No encryption key in your Keychain. Sign in to that browser once, then retry.",
        "browserRunning" => "Quit the browser first so its cookie database can be read.",
        "unsupportedPlatform" => "Importing from this browser isn't possible on this platform.",
        "keychainUnavailable" => "The system keyring could not be accessed. Make sure your desktop keyring is running and unlocked, then retry.",
        "unknownSourceProfile" => "That browser profile no longer exists.",
        "sessionUnavailable" => "The target profile could not be opened.",
        _ => "The browser's cookie database could not be read.",
    }
}

/// `isRetryableReason`.
pub fn retryable(reason: &str) -> bool {
    matches!(reason, "needsKeychainApproval" | "keychainItemMissing" | "keychainUnavailable" | "readFailed" | "sessionUnavailable")
}

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub directory: String,
    pub name: String,
    pub cookies: Option<usize>,
}

fn root(source: &Source) -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let join = |segments: &[&str]| segments.iter().fold(home.clone(), |p, s| p.join(s));
    if cfg!(target_os = "macos") {
        return (!source.mac.is_empty()).then(|| join(source.mac));
    }
    if !cfg!(target_os = "linux") {
        return None;
    }
    let candidates: Vec<PathBuf> = source.linux.iter().map(|s| join(s)).collect();
    candidates.iter().find(|p| p.is_dir()).or(candidates.first()).cloned()
}

/// The browsers installed here, in menu order.
pub fn installed() -> Vec<&'static Source> {
    SOURCES.iter().filter(|s| root(s).is_some_and(|r| r.is_dir())).collect()
}

fn safe(directory: &str) -> bool {
    !directory.is_empty() && directory != "." && directory != ".." && !directory.contains(['/', '\\'])
}

/// A profile's directory: under the root, or absolute where Firefox's `IsRelative=0` or a Safari
/// profile's store put it.
fn profile_path(root: &Path, directory: &str) -> PathBuf {
    if Path::new(directory).is_absolute() { PathBuf::from(directory) } else { root.join(directory) }
}

fn cookie_db(engine: Engine, profile: &Path) -> Option<PathBuf> {
    let candidates = match engine {
        // Prefer Chromium 96+'s Network/Cookies; upgrades leave the pre-96 Cookies file behind.
        Engine::Chromium => vec![profile.join("Network").join("Cookies"), profile.join("Cookies")],
        Engine::Firefox => vec![profile.join("cookies.sqlite")],
        Engine::Safari => vec![profile.join("Cookies.binarycookies")],
    };
    candidates.into_iter().find(|p| p.is_file())
}

/// The listed profile `directory` names, so an import reads only a profile the list offered.
fn listed_db(source: &Source, directory: &str) -> Result<PathBuf, &'static str> {
    let root = root(source).ok_or("unsupportedPlatform")?;
    if !profiles(source).iter().any(|p| p.directory == directory) {
        return Err("unknownSourceProfile");
    }
    cookie_db(source.engine, &profile_path(&root, directory)).ok_or("unknownSourceProfile")
}

/// `parseFirefoxProfiles`: `[ProfileN]` blocks' names and paths; relative paths stay inside the
/// root, and `IsRelative=0` permits an absolute one elsewhere.
fn firefox_profiles(ini: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut current: Option<(Option<String>, Option<String>, Option<String>)> = None;
    let flush = |current: &mut Option<(Option<String>, Option<String>, Option<String>)>, out: &mut Vec<(String, String)>| {
        let Some((name, Some(path), relative)) = current.take() else { return };
        let directory = match relative.as_deref() {
            None | Some("1") => {
                let p = Path::new(&path);
                let inside = !p.is_absolute() && p.components().all(|c| matches!(c, std::path::Component::Normal(_) | std::path::Component::CurDir));
                inside.then_some(path)
            }
            Some("0") => Path::new(&path).is_absolute().then_some(path),
            _ => None,
        };
        if let Some(directory) = directory.filter(|d| !d.contains('\0')) {
            let name = name.map(|n| n.trim().to_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| directory.clone());
            out.push((directory, name));
        }
    };
    for line in ini.lines().map(str::trim) {
        if line.starts_with('[') {
            flush(&mut current, &mut out);
            let section = line.trim_start_matches('[').trim_end_matches(']');
            let numbered = section.len() > 7 && section[..7].eq_ignore_ascii_case("profile") && section[7..].bytes().all(|b| b.is_ascii_digit());
            current = numbered.then(|| (None, None, None));
            continue;
        }
        let (Some(entry), Some((key, value))) = (current.as_mut(), line.split_once('=')) else { continue };
        match key.trim() {
            "Name" => entry.0 = Some(value.to_owned()),
            "Path" => entry.1 = Some(value.trim().to_owned()),
            "IsRelative" => entry.2 = Some(value.trim().to_owned()),
            _ => {}
        }
    }
    flush(&mut current, &mut out);
    out
}

/// Firefox's declared profiles that have cookies; if none does, any profile folder that has.
fn gecko_profiles(root: &Path) -> Vec<(String, String)> {
    let declared = std::fs::read_to_string(root.join("profiles.ini")).map(|ini| firefox_profiles(&ini)).unwrap_or_default();
    let found: Vec<_> = declared.into_iter().filter(|(d, _)| cookie_db(Engine::Firefox, &profile_path(root, d)).is_some()).collect();
    if !found.is_empty() {
        return found;
    }
    let mut scanned = Vec::new();
    for base in [root.to_path_buf(), root.join("Profiles")] {
        let mut dirs: Vec<String> = std::fs::read_dir(&base).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
        dirs.sort();
        let prefix = if base == root { String::new() } else { "Profiles/".to_owned() };
        for d in dirs {
            let directory = format!("{prefix}{d}");
            if safe(&d) && cookie_db(Engine::Firefox, &root.join(&directory)).is_some() {
                scanned.push((directory.clone(), d));
            }
        }
    }
    scanned
}

/// `listSafariProfiles`: the default jar, then each named profile from `SafariTabs.db` (or, with
/// that unreadable, each store on disk that holds a jar).
fn safari_profiles(root: &Path) -> Vec<(String, String)> {
    let Some(library) = root.parent() else { return Vec::new() };
    let uuid = |v: &str| {
        let parts: Vec<&str> = v.split('-').collect();
        parts.iter().map(|p| p.len()).eq([8, 4, 4, 4, 12]) && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_hexdigit()))
    };
    let declared: Vec<(String, String)> = std::fs::read(library.join("Safari").join("SafariTabs.db"))
        .ok()
        .and_then(|data| {
            let db = Db::open(&data)?;
            let t = db.table("bookmarks")?;
            let mut rows: Vec<_> = t
                .rows
                .iter()
                .filter(|r| t.get(r, "parent").int() == 0 && t.get(r, "type").int() == 1 && t.get(r, "subtype").int() == 2 && t.get(r, "deleted").int() == 0)
                .map(|r| (t.get(r, "order_index").int(), t.get(r, "title").text(), t.get(r, "external_uuid").text()))
                .collect();
            rows.sort_by_key(|r| r.0);
            Some(rows.into_iter().map(|(_, title, uuid)| (uuid, title.trim().to_owned())).collect())
        })
        .unwrap_or_default();
    let default_name = declared.iter().find(|(u, _)| u == "DefaultProfile").map(|(_, t)| if t.is_empty() { "Personal".to_owned() } else { t.clone() });
    let mut out = vec![(".".to_owned(), default_name.unwrap_or_else(|| "Safari".to_owned()))];
    let stores = library.join("WebKit").join("WebsiteDataStore");
    let store = |u: &str| stores.join(u.to_lowercase()).join("Cookies").to_string_lossy().into_owned();
    for (u, title) in declared.iter().filter(|(u, _)| uuid(u)) {
        out.push((store(u), if title.is_empty() { u.clone() } else { title.clone() }));
    }
    // With readable metadata, deleted profiles left on disk stay gone.
    if declared.is_empty() {
        let mut entries: Vec<String> = std::fs::read_dir(&stores).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).filter(|e| uuid(e)).collect();
        entries.sort();
        for e in entries {
            if stores.join(&e).join("Cookies").join("Cookies.binarycookies").is_file() {
                out.push((store(&e), e));
            }
        }
    }
    out
}

/// How many cookies a profile's database holds, as the list shows; `None` when unreadable.
fn count(engine: Engine, db: &Path) -> Option<usize> {
    match engine {
        Engine::Chromium => Some(Db::open(&std::fs::read(db).ok()?)?.table("cookies")?.rows.len()),
        Engine::Firefox => {
            let data = gecko_snapshot(db)?;
            let t = Db::open(&data)?.table("moz_cookies")?;
            // Only the default container: Rune's Chrome can't keep containers apart.
            Some(t.rows.iter().filter(|r| t.get(r, "originAttributes").text().is_empty()).count())
        }
        Engine::Safari => parse_binary_cookies(&std::fs::read(db).ok()?).ok().map(|c| c.len()),
    }
}

/// Profiles from `Local State`, or every directory holding a cookie database.
pub fn profiles(source: &Source) -> Vec<Profile> {
    let Some(root) = root(source) else { return Vec::new() };
    if source.engine != Engine::Chromium {
        let listed = if source.engine == Engine::Firefox { gecko_profiles(&root) } else { safari_profiles(&root) };
        return listed
            .into_iter()
            .filter_map(|(directory, name)| {
                let db = cookie_db(source.engine, &profile_path(&root, &directory))?;
                Some(Profile { cookies: count(source.engine, &db), directory, name })
            })
            .collect();
    }
    let state = std::fs::read_to_string(root.join("Local State")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let mut named: Vec<(String, String)> = state
        .as_ref()
        .and_then(|s| s["profile"]["info_cache"].as_object())
        .into_iter()
        .flatten()
        .map(|(dir, info)| (dir.clone(), info["name"].as_str().map(str::trim).filter(|n| !n.is_empty()).unwrap_or(dir).to_owned()))
        .collect();
    if named.is_empty() {
        let mut dirs: Vec<String> = std::fs::read_dir(&root).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
        dirs.sort();
        named = dirs.into_iter().map(|d| (d.clone(), d)).collect();
    }
    named
        .into_iter()
        .filter(|(dir, _)| safe(dir))
        .filter_map(|(directory, name)| {
            let db = cookie_db(Engine::Chromium, &root.join(&directory))?;
            Some(Profile { cookies: count(Engine::Chromium, &db), directory, name })
        })
        .collect()
}

/// Chrome's `SingletonLock` links to `host-pid` while a browser holds the profile; Firefox
/// holds each profile's `lock` link or `.parentlock`. Safari writes its jar atomically.
pub fn running(source: &Source) -> bool {
    let Some(root) = root(source) else { return false };
    match source.engine {
        Engine::Safari => false,
        Engine::Chromium => match std::fs::read_link(root.join("SingletonLock")) {
            Ok(target) => lock_held(&target.to_string_lossy(), &hostname(), alive),
            Err(e) => e.kind() != std::io::ErrorKind::NotFound,
        },
        Engine::Firefox => gecko_profiles(&root).iter().any(|(d, _)| gecko_held(&profile_path(&root, d))),
    }
}

/// `firefoxProfileIsHeld`: Firefox leaves its lock files after exit, so test the lock itself.
fn gecko_held(profile: &Path) -> bool {
    if let Ok(target) = std::fs::read_link(profile.join("lock"))
        && gecko_link_held(&target.to_string_lossy(), &local_addresses(), alive)
    {
        return true;
    }
    let parent = profile.join(".parentlock");
    parent.is_file() && fcntl_held(&parent)
}

/// `firefoxSymlinkLockIsHeld`: the link reads `address:+pid`; only a local owner's pid means
/// anything, and anything unreadable counts as held.
fn gecko_link_held(target: &str, local: &[String], alive: impl Fn(u32) -> bool) -> bool {
    let Some((owner, pid)) = target.rsplit_once(':') else { return true };
    if !local.iter().any(|a| a == owner) {
        return true;
    }
    // A `+` marks an fcntl-holding owner; the pid follows either way.
    let pid = pid.strip_prefix('+').unwrap_or(pid);
    match pid.parse::<u32>() {
        Ok(n) if n > 0 && pid.bytes().all(|b| b.is_ascii_digit()) => alive(n),
        _ => true,
    }
}

/// Loopback and this machine's own addresses, which Firefox writes as the lock's owner.
fn local_addresses() -> Vec<String> {
    let mut out = vec!["127.0.0.1".to_owned(), "127.0.1.1".to_owned(), "::1".to_owned()];
    if cfg!(target_os = "linux")
        && let Ok(o) = Command::new("hostname").arg("-I").output()
    {
        out.extend(String::from_utf8_lossy(&o.stdout).split_whitespace().map(str::to_owned));
    }
    if cfg!(target_os = "macos")
        && let Ok(o) = Command::new("ifconfig").output()
    {
        let text = String::from_utf8_lossy(&o.stdout);
        out.extend(text.split_whitespace().collect::<Vec<_>>().windows(2).filter(|w| w[0] == "inet" || w[0] == "inet6").map(|w| w[1].split('%').next().unwrap_or(w[1]).to_owned()));
    }
    out
}

/// Electron's `posixLockIsHeld`: try Firefox's fcntl lock from a short Python probe. With no
/// interpreter the probe can't tell, and "free" keeps the import possible.
fn fcntl_held(path: &Path) -> bool {
    const PROBE: &str = "import fcntl,os,sys\nfd=os.open(sys.argv[1],os.O_WRONLY)\ntry:\n  fcntl.lockf(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)\nexcept BlockingIOError:\n  print('held')\nelse:\n  print('free')";
    for interpreter in ["/usr/bin/python3", "python3"] {
        let Ok(out) = Command::new(interpreter).arg("-c").arg(PROBE).arg(path).stdin(Stdio::null()).stderr(Stdio::null()).output() else { continue };
        match String::from_utf8_lossy(&out.stdout).trim() {
            "held" => return true,
            "free" => return false,
            _ => {}
        }
    }
    false
}

fn lock_held(target: &str, host: &str, alive: impl Fn(u32) -> bool) -> bool {
    let Some((h, pid)) = target.rsplit_once('-') else { return true };
    if h.is_empty() || pid.is_empty() || !pid.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    match pid.parse::<u32>() {
        // Another host's pid may come from a shared home; never call it stale.
        Ok(pid) if pid > 0 && h == host => alive(pid),
        _ => true,
    }
}

fn alive(pid: u32) -> bool {
    if Path::new("/proc/self").exists() {
        return Path::new(&format!("/proc/{pid}")).exists();
    }
    // Only "no such process" proves a stale lock; a permission error still means it runs.
    match Command::new("kill").args(["-0", &pid.to_string()]).stdout(Stdio::null()).output() {
        Ok(o) => o.status.success() || !String::from_utf8_lossy(&o.stderr).to_lowercase().contains("no such process"),
        Err(_) => true,
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| Command::new("hostname").output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()))
        .unwrap_or_default()
        .trim()
        .to_owned()
}

#[derive(Default)]
struct Keys {
    v10: Option<[u8; 16]>,
    v11: Option<[u8; 16]>,
    /// Why the v11 key is missing, reported when every cookie needed it.
    v11_error: Option<&'static str>,
    /// Some Linux clients encrypt with an empty passphrase (crbug.com/1195256).
    empty: Option<[u8; 16]>,
}

pub(crate) fn derive(passphrase: &[u8], iterations: u32) -> [u8; 16] {
    let mut key = [0; 16];
    let iterations = std::num::NonZeroU32::new(iterations).expect("iterations > 0");
    ring::pbkdf2::derive(ring::pbkdf2::PBKDF2_HMAC_SHA1, iterations, b"saltysalt", passphrase, &mut key);
    key
}

fn keys(source: &Source) -> Result<Keys, &'static str> {
    if cfg!(target_os = "macos") {
        let secret = keychain(source.service, source.account)?;
        return Ok(Keys { v10: Some(derive(secret.as_bytes(), 1003)), ..Keys::default() });
    }
    if !cfg!(target_os = "linux") {
        return Err("unsupportedPlatform");
    }
    let mut keys = Keys { v10: Some(derive(b"peanuts", 1)), empty: Some(derive(b"", 1)), ..Keys::default() };
    if !source.secret.is_empty() {
        match linux_secret(source.secret) {
            Ok(secret) => keys.v11 = Some(derive(secret.as_bytes(), 1)),
            // A missing keyring must not block v10 cookies.
            Err(e) => keys.v11_error = Some(e),
        }
    }
    Ok(keys)
}

fn keychain(service: &str, account: &str) -> Result<String, &'static str> {
    keychain_lookup(&["-s", service, "-a", account])
}

/// A generic password by its service alone, when the account name isn't known.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn keychain_service(service: &str) -> Result<String, &'static str> {
    keychain_lookup(&["-s", service])
}

fn keychain_lookup(query: &[&str]) -> Result<String, &'static str> {
    let out = Command::new("security").args(["find-generic-password", "-w"]).args(query).output().map_err(|_| "keychainUnavailable")?;
    match out.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned()).filter(|s| !s.is_empty()).ok_or("keychainItemMissing"),
        // errSecItemNotFound
        Some(44) => Err("keychainItemMissing"),
        _ => Err("needsKeychainApproval"),
    }
}

pub(crate) fn linux_secret(application: &str) -> Result<String, &'static str> {
    let out = Command::new("secret-tool").args(["lookup", "application", application]).stdin(Stdio::null()).output().map_err(|_| "keychainUnavailable")?;
    let secret = String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned();
    if out.status.success() && !secret.is_empty() { Ok(secret) } else { Err("keychainItemMissing") }
}

fn decrypt(encrypted: &[u8], keys: &Keys, host: &str, version: i64) -> Option<String> {
    let (prefix, payload) = (encrypted.get(..3)?, &encrypted[3..]);
    let primary = match prefix {
        b"v10" => keys.v10?,
        b"v11" => keys.v11?,
        _ => return None,
    };
    [Some(primary), keys.empty].into_iter().flatten().find_map(|key| decrypt_cbc(payload, &key, host, version))
}

pub(crate) fn decrypt_cbc(payload: &[u8], key: &[u8; 16], host: &str, version: i64) -> Option<String> {
    if payload.is_empty() || payload.len() % 16 != 0 {
        return None;
    }
    let aes = Aes::new(key);
    let mut previous = [0x20; 16];
    let mut plain = Vec::with_capacity(payload.len());
    for chunk in payload.chunks(16) {
        let block: [u8; 16] = chunk.try_into().ok()?;
        let out = aes.decrypt(block);
        plain.extend(out.iter().zip(previous).map(|(a, b)| a ^ b));
        previous = block;
    }
    let pad = *plain.last()? as usize;
    if pad == 0 || pad > 16 || plain[plain.len() - pad..].iter().any(|&b| b as usize != pad) {
        return None;
    }
    plain.truncate(plain.len() - pad);
    // Chromium 127+ (schema 24) binds a value to its host with the host key's SHA-256.
    let value = if version >= 24 {
        let digest = ring::digest::digest(&ring::digest::SHA256, host.as_bytes());
        if plain.len() < 32 || plain[..32] != *digest.as_ref() {
            return None;
        }
        &plain[32..]
    } else {
        &plain[..]
    };
    Some(String::from_utf8_lossy(value).into_owned())
}

/// OSCrypt's CBC with a space IV and PKCS #7 padding, as safeStorage writes after its prefix.
pub(crate) fn encrypt_cbc(plain: &[u8], key: &[u8; 16]) -> Vec<u8> {
    let aes = Aes::new(key);
    let pad = 16 - plain.len() % 16;
    let mut padded = plain.to_vec();
    padded.extend(std::iter::repeat_n(pad as u8, pad));
    let mut previous = [0x20u8; 16];
    let mut out = Vec::with_capacity(padded.len());
    for chunk in padded.chunks(16) {
        let mut block = [0u8; 16];
        block.iter_mut().zip(chunk.iter().zip(previous)).for_each(|(b, (p, v))| *b = p ^ v);
        previous = aes.encrypt(block);
        out.extend_from_slice(&previous);
    }
    out
}

/// AES-128 (FIPS-197), the one primitive `ring` does not expose.
struct Aes {
    keys: [[u8; 16]; 11],
}

fn xtime(b: u8) -> u8 {
    (b << 1) ^ if b & 0x80 != 0 { 0x1b } else { 0 }
}

fn gmul(mut a: u8, mut b: u8) -> u8 {
    let mut p = 0;
    while b != 0 {
        if b & 1 != 0 {
            p ^= a;
        }
        a = xtime(a);
        b >>= 1;
    }
    p
}

fn sboxes() -> &'static ([u8; 256], [u8; 256]) {
    static BOXES: OnceLock<([u8; 256], [u8; 256])> = OnceLock::new();
    BOXES.get_or_init(|| {
        let (mut sbox, mut inverse) = ([0u8; 256], [0u8; 256]);
        for i in 0..256 {
            let x = if i == 0 { 0 } else { (1..=255u8).find(|&y| gmul(i as u8, y) == 1).unwrap_or(0) };
            let (mut r, mut y) = (x, x);
            for _ in 0..4 {
                y = y.rotate_left(1);
                r ^= y;
            }
            r ^= 0x63;
            sbox[i] = r;
            inverse[r as usize] = i as u8;
        }
        (sbox, inverse)
    })
}

impl Aes {
    fn new(key: &[u8; 16]) -> Aes {
        let sbox = &sboxes().0;
        let mut w = [[0u8; 4]; 44];
        for (i, word) in w.iter_mut().take(4).enumerate() {
            word.copy_from_slice(&key[4 * i..4 * i + 4]);
        }
        let mut rcon = 1u8;
        for i in 4..44 {
            let mut t = w[i - 1];
            if i % 4 == 0 {
                t = [sbox[t[1] as usize] ^ rcon, sbox[t[2] as usize], sbox[t[3] as usize], sbox[t[0] as usize]];
                rcon = xtime(rcon);
            }
            for j in 0..4 {
                w[i][j] = w[i - 4][j] ^ t[j];
            }
        }
        let mut keys = [[0u8; 16]; 11];
        for (r, round) in keys.iter_mut().enumerate() {
            for c in 0..4 {
                round[4 * c..4 * c + 4].copy_from_slice(&w[4 * r + c]);
            }
        }
        Aes { keys }
    }

    fn encrypt(&self, mut s: [u8; 16]) -> [u8; 16] {
        let sbox = &sboxes().0;
        let add = |s: &mut [u8; 16], k: &[u8; 16]| s.iter_mut().zip(k).for_each(|(a, b)| *a ^= b);
        add(&mut s, &self.keys[0]);
        for round in 1..=10 {
            let old = s;
            for c in 0..4 {
                for r in 0..4 {
                    s[r + 4 * c] = sbox[old[r + 4 * ((c + r) % 4)] as usize];
                }
            }
            if round != 10 {
                for c in 0..4 {
                    let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                    let m = |x: [u8; 4]| gmul(a[0], x[0]) ^ gmul(a[1], x[1]) ^ gmul(a[2], x[2]) ^ gmul(a[3], x[3]);
                    s[4 * c] = m([2, 3, 1, 1]);
                    s[4 * c + 1] = m([1, 2, 3, 1]);
                    s[4 * c + 2] = m([1, 1, 2, 3]);
                    s[4 * c + 3] = m([3, 1, 1, 2]);
                }
            }
            add(&mut s, &self.keys[round]);
        }
        s
    }

    fn decrypt(&self, mut s: [u8; 16]) -> [u8; 16] {
        let inverse = &sboxes().1;
        let add = |s: &mut [u8; 16], k: &[u8; 16]| s.iter_mut().zip(k).for_each(|(a, b)| *a ^= b);
        let unshift_unsub = |s: &mut [u8; 16]| {
            let old = *s;
            for c in 0..4 {
                for r in 0..4 {
                    s[r + 4 * c] = inverse[old[r + 4 * ((c + 4 - r) % 4)] as usize];
                }
            }
        };
        add(&mut s, &self.keys[10]);
        for round in (1..10).rev() {
            unshift_unsub(&mut s);
            add(&mut s, &self.keys[round]);
            for c in 0..4 {
                let a = [s[4 * c], s[4 * c + 1], s[4 * c + 2], s[4 * c + 3]];
                let m = |x: [u8; 4]| gmul(a[0], x[0]) ^ gmul(a[1], x[1]) ^ gmul(a[2], x[2]) ^ gmul(a[3], x[3]);
                s[4 * c] = m([14, 11, 13, 9]);
                s[4 * c + 1] = m([9, 14, 11, 13]);
                s[4 * c + 2] = m([13, 9, 14, 11]);
                s[4 * c + 3] = m([11, 13, 9, 14]);
            }
        }
        unshift_unsub(&mut s);
        add(&mut s, &self.keys[0]);
        s
    }
}

/// A SQLite value, as the record format stores it.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Val {
    fn int(&self) -> i64 {
        match self {
            Val::Int(i) => *i,
            Val::Real(f) => *f as i64,
            Val::Text(t) => t.trim().parse().unwrap_or(0),
            _ => 0,
        }
    }

    fn text(&self) -> String {
        match self {
            Val::Text(t) => t.clone(),
            Val::Int(i) => i.to_string(),
            Val::Blob(b) => String::from_utf8_lossy(b).into_owned(),
            _ => String::new(),
        }
    }

    fn bytes(&self) -> &[u8] {
        match self {
            Val::Blob(b) => b,
            Val::Text(t) => t.as_bytes(),
            _ => &[],
        }
    }
}

struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Val>>,
}

impl Table {
    fn get<'a>(&'a self, row: &'a [Val], column: &str) -> &'a Val {
        static NULL: Val = Val::Null;
        self.columns.iter().position(|c| c.eq_ignore_ascii_case(column)).and_then(|i| row.get(i)).unwrap_or(&NULL)
    }
}

/// Just enough of the SQLite file format to walk a rowid table's b-tree.
struct Db<'a> {
    data: &'a [u8],
    page: usize,
    usable: usize,
}

fn be(bytes: &[u8], at: usize, len: usize) -> Option<u64> {
    Some(bytes.get(at..at + len)?.iter().fold(0u64, |v, &b| (v << 8) | b as u64))
}

fn varint(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut v = 0u64;
    for i in 0..9 {
        let c = *bytes.get(at + i)?;
        if i == 8 {
            return Some(((v << 8) | c as u64, 9));
        }
        v = (v << 7) | (c & 0x7f) as u64;
        if c & 0x80 == 0 {
            return Some((v, i + 1));
        }
    }
    None
}

fn record(payload: &[u8]) -> Option<Vec<Val>> {
    let (header, mut at) = varint(payload, 0)?;
    let mut body = header as usize;
    let mut values = Vec::new();
    while at < header as usize {
        let (kind, n) = varint(payload, at)?;
        at += n;
        let (value, len) = match kind {
            0 => (Val::Null, 0),
            1..=6 => {
                let len = [0, 1, 2, 3, 4, 6, 8][kind as usize];
                let bytes = payload.get(body..body + len)?;
                let v = bytes.iter().fold(if bytes[0] & 0x80 != 0 { -1i64 } else { 0 }, |v, &b| (v << 8) | b as i64);
                (Val::Int(v), len)
            }
            7 => (Val::Real(f64::from_bits(be(payload, body, 8)?)), 8),
            8 => (Val::Int(0), 0),
            9 => (Val::Int(1), 0),
            k if k >= 12 && k % 2 == 0 => {
                let len = ((k - 12) / 2) as usize;
                (Val::Blob(payload.get(body..body + len)?.to_vec()), len)
            }
            k if k >= 13 => {
                let len = ((k - 13) / 2) as usize;
                (Val::Text(String::from_utf8_lossy(payload.get(body..body + len)?).into_owned()), len)
            }
            _ => return None,
        };
        body += len;
        values.push(value);
    }
    Some(values)
}

/// Column names from a `CREATE TABLE`, and which one aliases the rowid.
fn columns(sql: &str) -> (Vec<String>, Option<usize>) {
    let (Some(open), Some(close)) = (sql.find('('), sql.rfind(')')) else { return (Vec::new(), None) };
    let (mut parts, mut depth, mut start) = (Vec::new(), 0, open + 1);
    for (i, ch) in sql.char_indices().take_while(|(i, _)| *i < close).skip_while(|(i, _)| *i <= open) {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&sql[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&sql[start..close]);
    let (mut names, mut alias) = (Vec::new(), None);
    for def in parts {
        let Some(first) = def.split_whitespace().next() else { continue };
        if ["CONSTRAINT", "PRIMARY", "UNIQUE", "CHECK", "FOREIGN"].iter().any(|k| first.eq_ignore_ascii_case(k)) {
            continue;
        }
        let words: Vec<String> = def.split_whitespace().map(str::to_ascii_uppercase).collect();
        if words.get(1).is_some_and(|t| t == "INTEGER") && words.windows(2).any(|w| w[0] == "PRIMARY" && w[1] == "KEY") {
            alias = Some(names.len());
        }
        names.push(first.trim_matches(|c| matches!(c, '"' | '`' | '[' | ']')).to_owned());
    }
    (names, alias)
}

impl<'a> Db<'a> {
    fn open(data: &'a [u8]) -> Option<Db<'a>> {
        if data.len() < 100 || &data[..16] != b"SQLite format 3\0" {
            return None;
        }
        let page = match be(data, 16, 2)? {
            1 => 65536,
            n => n as usize,
        };
        let usable = page.checked_sub(data[20] as usize).filter(|u| *u >= 480)?;
        Some(Db { data, page, usable })
    }

    fn page(&self, n: u64) -> Option<&'a [u8]> {
        let start = (n as usize).checked_sub(1)? * self.page;
        self.data.get(start..start + self.page)
    }

    fn rows(&self, n: u64, out: &mut Vec<(i64, Vec<Val>)>, depth: u32) -> Option<()> {
        if depth > 32 {
            return None;
        }
        let page = self.page(n)?;
        let header = if n == 1 { 100 } else { 0 };
        let cells = be(page, header + 3, 2)? as usize;
        match *page.get(header)? {
            0x0d => {
                for i in 0..cells {
                    let at = be(page, header + 8 + 2 * i, 2)? as usize;
                    let (size, a) = varint(page, at)?;
                    let (rowid, b) = varint(page, at + a)?;
                    let payload = self.payload(page, at + a + b, size as usize)?;
                    out.push((rowid as i64, record(&payload)?));
                }
            }
            0x05 => {
                for i in 0..cells {
                    let at = be(page, header + 12 + 2 * i, 2)? as usize;
                    self.rows(be(page, at, 4)?, out, depth + 1)?;
                }
                self.rows(be(page, header + 8, 4)?, out, depth + 1)?;
            }
            _ => return None,
        }
        Some(())
    }

    fn payload(&self, page: &[u8], at: usize, size: usize) -> Option<Vec<u8>> {
        let u = self.usable;
        let max_local = u - 35;
        if size <= max_local {
            return page.get(at..at + size).map(<[u8]>::to_vec);
        }
        let min_local = (u - 12) * 32 / 255 - 23;
        let k = min_local + (size - min_local) % (u - 4);
        let local = if k <= max_local { k } else { min_local };
        let mut out = page.get(at..at + local)?.to_vec();
        let mut next = be(page, at + local, 4)?;
        for _ in 0..self.data.len() / self.page {
            if out.len() >= size {
                return Some(out);
            }
            let overflow = self.page(next)?;
            let take = (size - out.len()).min(u - 4);
            out.extend_from_slice(overflow.get(4..4 + take)?);
            next = be(overflow, 0, 4)?;
        }
        (out.len() >= size).then_some(out)
    }

    fn table(&self, name: &str) -> Option<Table> {
        let mut schema = Vec::new();
        self.rows(1, &mut schema, 0)?;
        let entry = schema.into_iter().map(|(_, r)| r).find(|r| r.first().map(Val::text).as_deref() == Some("table") && r.get(1).is_some_and(|n| n.text() == name))?;
        let (columns, alias) = columns(&entry.get(4)?.text());
        let mut raw = Vec::new();
        self.rows(entry.get(3)?.int() as u64, &mut raw, 0)?;
        let rows = raw
            .into_iter()
            .map(|(rowid, mut row)| {
                // Columns added later are absent from older records.
                row.resize(columns.len().max(row.len()), Val::Null);
                if let Some(i) = alias {
                    row[i] = Val::Int(rowid);
                }
                row
            })
            .collect();
        Some(Table { columns, rows })
    }
}

fn bare(host: &str) -> &str {
    host.strip_prefix('.').unwrap_or(host)
}

/// A CDP `CookieParam`, scoped as Electron's `cookieScope` does. `expires` is in UNIX seconds.
#[allow(clippy::too_many_arguments)]
fn cookie_param(host: &str, name: &str, value: &str, path: &str, expires: Option<i64>, secure: bool, http_only: bool, same_site: Option<&str>) -> Value {
    let bare_host = bare(host);
    let authority = if bare_host.contains(':') && !(bare_host.starts_with('[') && bare_host.ends_with(']')) { format!("[{bare_host}]") } else { bare_host.to_owned() };
    let scheme = if secure { "https" } else { "http" };
    let mut cookie = json!({ "url": format!("{scheme}://{authority}{path}"), "name": name, "value": value, "path": path, "secure": secure, "httpOnly": http_only });
    if host.starts_with('.') {
        cookie["domain"] = json!(host);
    }
    if let Some(expires) = expires {
        cookie["expires"] = json!(expires);
    }
    if let Some(same_site) = same_site {
        cookie["sameSite"] = json!(same_site);
    }
    cookie
}

/// Chrome's `samesite` column; unspecified (-1) stays unset.
fn chromium_same_site(value: i64) -> Option<&'static str> {
    match value {
        0 => Some("None"),
        1 => Some("Lax"),
        2 => Some("Strict"),
        _ => None,
    }
}

/// The database as SQLite would read it: the main file with its write-ahead log's committed
/// frames over it, which is what Electron's `VACUUM INTO` snapshot sees.
fn gecko_snapshot(db: &Path) -> Option<Vec<u8>> {
    let mut data = std::fs::read(db).ok()?;
    let mut wal = db.as_os_str().to_owned();
    wal.push("-wal");
    if let Ok(log) = std::fs::read(&wal) {
        apply_wal(&mut data, &log);
    }
    Some(data)
}

/// Applies a WAL's frames up to its last commit. Frames whose salts don't match the header are
/// left over from before the last checkpoint and are skipped.
fn apply_wal(data: &mut Vec<u8>, log: &[u8]) {
    let (Some(magic), Some(page)) = (be(log, 0, 4), be(log, 8, 4)) else { return };
    if magic & !1 != 0x377f_0682 || !(512..=65536).contains(&page) || !page.is_power_of_two() {
        return;
    }
    let page = page as usize;
    let salts = log.get(16..24);
    let mut committed: Vec<(usize, &[u8])> = Vec::new();
    let mut pending: Vec<(usize, &[u8])> = Vec::new();
    let mut size = None;
    let mut at = 32;
    while let (Some(header), Some(body)) = (log.get(at..at + 24), log.get(at + 24..at + 24 + page)) {
        if Some(&header[8..16]) != salts {
            break;
        }
        let number = be(header, 0, 4).unwrap_or(0) as usize;
        if number == 0 {
            break;
        }
        pending.push((number, body));
        let commit = be(header, 4, 4).unwrap_or(0) as usize;
        if commit > 0 {
            committed.append(&mut pending);
            size = Some(commit);
        }
        at += 24 + page;
    }
    let Some(pages) = size else { return };
    if be(data, 16, 2).map(|p| if p == 1 { 65536 } else { p as usize }) != Some(page) && !data.is_empty() {
        return;
    }
    data.resize(pages * page, 0);
    for (number, body) in committed {
        if number <= pages {
            data[(number - 1) * page..number * page].copy_from_slice(body);
        }
    }
}

/// Firefox's `sameSite`, with `rawSameSite` (schemas 10–14) keeping an undeclared value unset:
/// mapping it to None would let the cookie go cross-site.
fn gecko_same_site(value: &Val, raw: &Val) -> Option<&'static str> {
    match (value, raw.int()) {
        (Val::Null, _) => None,
        (v, 0) if v.int() == 1 && !matches!(raw, Val::Null) => None,
        (v, _) => match v.int() {
            0 => Some("None"),
            1 => Some("Lax"),
            2 => Some("Strict"),
            _ => None,
        },
    }
}

/// `readFirefoxCookies`: the default container's cookies. Schema 16+ stores expiry in
/// milliseconds, older ones in seconds.
fn read_gecko(data: &[u8]) -> Result<Read, &'static str> {
    let db = Db::open(data).ok_or("readFailed")?;
    let schema = be(data, 60, 4).unwrap_or(0);
    let table = db.table("moz_cookies").ok_or("readFailed")?;
    let mut read = Read { cookies: Vec::new(), skipped: 0, domains: Vec::new() };
    for row in &table.rows {
        let col = |name| table.get(row, name);
        if !col("originAttributes").text().is_empty() {
            continue;
        }
        let host = col("host").text();
        let expiry = col("expiry").int();
        let expires = (expiry > 0).then(|| if schema >= 16 { expiry / 1000 } else { expiry });
        let raw = if (10..=14).contains(&schema) { col("rawSameSite").clone() } else { Val::Null };
        let secure = col("isSecure").int() == 1;
        let cookie = cookie_param(&host, &col("name").text(), &col("value").text(), &col("path").text(), expires, secure, col("isHttpOnly").int() == 1, gecko_same_site(col("sameSite"), &raw));
        read.cookies.push((cookie, bare(&host).to_owned()));
    }
    Ok(read)
}

/// Safari's timestamps count seconds from 2001-01-01.
const APPLE_EPOCH_OFFSET: i64 = 978_307_200;

/// `parseBinaryCookies`: `Cookies.binarycookies`, a big-endian page index over little-endian
/// pages of records. Anything out of bounds or overlapping fails the whole file rather than
/// fabricating cookies.
fn parse_binary_cookies(data: &[u8]) -> Result<Vec<(Value, String)>, &'static str> {
    const PAGE_HEADER: usize = 12;
    const RECORD_HEADER: usize = 56;
    let le = |b: &[u8], at: usize| b.get(at..at + 4).map(|v| u32::from_le_bytes([v[0], v[1], v[2], v[3]]) as usize).ok_or("readFailed");
    let c_str = |b: &[u8], at: usize| {
        let tail = &b[at..];
        String::from_utf8_lossy(&tail[..tail.iter().position(|c| *c == 0).unwrap_or(tail.len())]).into_owned()
    };
    if data.len() < 8 || &data[..4] != b"cook" {
        return Err("readFailed");
    }
    let pages = be(data, 4, 4).ok_or("readFailed")? as usize;
    if 8 + pages * 4 > data.len() {
        return Err("readFailed");
    }
    let mut cookies = Vec::new();
    let mut start = 8 + pages * 4;
    for i in 0..pages {
        let size = be(data, 8 + i * 4, 4).ok_or("readFailed")? as usize;
        let page = data.get(start..start + size).filter(|_| size >= PAGE_HEADER).ok_or("readFailed")?;
        start += size;
        let count = le(page, 4)?;
        let table_end = PAGE_HEADER + count * 4;
        if table_end > page.len() {
            return Err("readFailed");
        }
        let mut taken: Vec<(usize, usize)> = Vec::new();
        for j in 0..count {
            let at = le(page, 8 + j * 4)?;
            if at < table_end || at + RECORD_HEADER > page.len() {
                return Err("readFailed");
            }
            let end = at + le(page, at)?;
            if end < at + RECORD_HEADER || end > page.len() || taken.iter().any(|&(s, e)| at < e && end > s) {
                return Err("readFailed");
            }
            taken.push((at, end));
            let record = &page[at..end];
            let flags = le(record, 8)?;
            let offsets = [le(record, 16)?, le(record, 20)?, le(record, 24)?, le(record, 28)?];
            if offsets.iter().any(|&o| o < RECORD_HEADER || o >= record.len()) {
                return Err("readFailed");
            }
            let expiry = f64::from_le_bytes(record[40..48].try_into().map_err(|_| "readFailed")?);
            let [domain, name, path, value] = offsets.map(|o| c_str(record, o));
            if domain.is_empty() || name.is_empty() {
                continue;
            }
            let path = if path.is_empty() { "/".to_owned() } else { path };
            let expires = (expiry > 0.0).then(|| expiry.floor() as i64 + APPLE_EPOCH_OFFSET);
            // Safari's SameSite encoding is undocumented; guessing None would widen the scope.
            let cookie = cookie_param(&domain, &name, &value, &path, expires, flags & 1 != 0, flags & 4 != 0, Some("Lax"));
            cookies.push((cookie, bare(&domain).to_owned()));
        }
    }
    // Pages end with an 8-byte checksum and an optional length-prefixed plist.
    let trailer = data.len() - start;
    let valid = trailer == 0 || trailer == 8 || (trailer >= 12 && be(data, start + 8, 4).is_some_and(|n| trailer == 12 + n as usize));
    if !valid {
        return Err("readFailed");
    }
    Ok(cookies)
}

/// Reading the jar failed: macOS's privacy protection answers EPERM, which Full Disk Access
/// fixes; EACCES is an ordinary denial it doesn't.
fn safari_read(path: &Path) -> Result<Vec<u8>, &'static str> {
    std::fs::read(path).map_err(|e| if e.raw_os_error() == Some(1) { "needsFullDiskAccess" } else { "readFailed" })
}

/// Whether any Safari jar is readable now (`safariPermissionCheck`).
pub fn safari_access() -> bool {
    let Some(source) = SOURCES.iter().find(|s| s.engine == Engine::Safari) else { return false };
    let Some(root) = root(source) else { return false };
    let jars: Vec<PathBuf> = std::iter::once(".".to_owned()).chain(safari_profiles(&root).into_iter().map(|(d, _)| d)).filter_map(|d| cookie_db(Engine::Safari, &profile_path(&root, &d))).collect();
    jars.iter().any(|jar| std::fs::File::open(jar).is_ok())
}

/// Safari is installed but its default jar is unreadable for lack of Full Disk Access.
pub fn needs_full_disk_access(source: &Source) -> bool {
    source.engine == Engine::Safari
        && root(source).and_then(|r| cookie_db(Engine::Safari, &r)).is_some_and(|jar| std::fs::File::open(jar).is_err_and(|e| e.raw_os_error() == Some(1)))
}

struct Read {
    /// Each cookie with the bare host a failed write is reported under.
    cookies: Vec<(Value, String)>,
    skipped: usize,
    domains: Vec<String>,
}

fn note(domains: &mut Vec<String>, host: &str) {
    if !domains.iter().any(|d| d == host) {
        domains.push(host.to_owned());
    }
}

fn read_cookies(data: &[u8], keys: &Keys) -> Result<Read, &'static str> {
    let db = Db::open(data).ok_or("readFailed")?;
    let version = db.table("meta").and_then(|m| m.rows.iter().find(|r| m.get(r, "key").text() == "version").map(|r| m.get(r, "value").int())).unwrap_or(0);
    let table = db.table("cookies").ok_or("readFailed")?;
    let mut read = Read { cookies: Vec::new(), skipped: 0, domains: Vec::new() };
    let mut needs_v11 = false;
    for row in &table.rows {
        let col = |name| table.get(row, name);
        let host = col("host_key").text();
        let encrypted = col("encrypted_value").bytes();
        // Partitioned (CHIPS) cookies belong to one top-level site; Electron skips them too.
        let partitioned = !col("top_frame_site_key").text().is_empty();
        needs_v11 |= !partitioned && encrypted.starts_with(b"v11");
        let value = if partitioned {
            None
        } else if encrypted.is_empty() {
            Some(col("value").text())
        } else {
            decrypt(encrypted, keys, &host, version)
        };
        let Some(value) = value else {
            read.skipped += 1;
            note(&mut read.domains, bare(&host));
            continue;
        };
        // Chrome stores expiry in microseconds since 1601.
        let seconds = col("expires_utc").int() / 1_000_000;
        let expires = (seconds > 0).then(|| seconds - 11_644_473_600);
        let cookie = cookie_param(&host, &col("name").text(), &value, &col("path").text(), expires, col("is_secure").int() == 1, col("is_httponly").int() == 1, chromium_same_site(col("samesite").int()));
        read.cookies.push((cookie, bare(&host).to_owned()));
    }
    if read.cookies.is_empty() && needs_v11 {
        if let Some(e) = keys.v11_error {
            return Err(e);
        }
    }
    Ok(read)
}

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Reading,
    /// Chrome is taking the cookies; `left` writes are still in flight.
    Writing { left: usize, imported: usize, skipped: usize, domains: Vec<String> },
    Done { imported: usize, skipped: usize, domains: Vec<String> },
    Failed(&'static str),
}

pub type Job = Arc<Mutex<Stage>>;

/// One cookie write finished; the job is done once none are left.
pub fn written(job: &Job, ok: bool, host: &str) -> bool {
    let mut stage = job.lock().unwrap();
    let Stage::Writing { left, imported, skipped, domains } = &mut *stage else { return false };
    if ok {
        *imported += 1;
    } else {
        *skipped += 1;
        note(domains, host);
    }
    *left -= 1;
    if *left > 0 {
        return false;
    }
    let mut domains = std::mem::take(domains);
    domains.truncate(20);
    *stage = Stage::Done { imported: *imported, skipped: *skipped, domains };
    true
}

/// Reads `directory`'s cookies off the UI thread (the Keychain may prompt), then hands them to
/// Chrome through `tx`.
pub fn start(source: &'static Source, directory: String, tx: Sender<Cmd>, waker: Waker) -> Job {
    let job: Job = Arc::new(Mutex::new(Stage::Reading));
    let shared = job.clone();
    std::thread::spawn(move || {
        let read = (|| {
            if running(source) {
                return Err("browserRunning");
            }
            let db = listed_db(source, &directory)?;
            match source.engine {
                Engine::Chromium => {
                    let keys = keys(source)?;
                    let data = std::fs::read(db).map_err(|_| "readFailed")?;
                    read_cookies(&data, &keys)
                }
                Engine::Firefox => read_gecko(&gecko_snapshot(&db).ok_or("readFailed")?),
                Engine::Safari => {
                    let cookies = parse_binary_cookies(&safari_read(&db)?)?;
                    Ok(Read { cookies, skipped: 0, domains: Vec::new() })
                }
            }
        })();
        let next = match read {
            Err(reason) => Stage::Failed(reason),
            Ok(mut read) if read.cookies.is_empty() => {
                read.domains.truncate(20);
                Stage::Done { imported: 0, skipped: read.skipped, domains: read.domains }
            }
            Ok(read) => {
                *shared.lock().unwrap() = Stage::Writing { left: read.cookies.len(), imported: 0, skipped: read.skipped, domains: read.domains };
                match tx.send(Cmd::SetCookies(read.cookies, shared.clone())) {
                    Ok(()) => return,
                    Err(_) => Stage::Failed("sessionUnavailable"),
                }
            }
        };
        *shared.lock().unwrap() = next;
        waker();
    });
    job
}

/// `formatSkippedDomains`.
pub fn skipped_domains(domains: &[String]) -> String {
    match domains {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] if domains.len() <= 3 => format!("{} and {last}", init.join(", ")),
        _ => format!("{} and {} more", domains[..3].join(", "), domains.len() - 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_encrypts_the_fips_197_vector_and_cbc_round_trips() {
        let key: [u8; 16] = std::array::from_fn(|i| i as u8);
        let plain: [u8; 16] = std::array::from_fn(|i| (i as u8) * 0x11);
        let cipher = Aes::new(&key).encrypt(plain);
        assert_eq!(cipher, [0x69, 0xc4, 0xe0, 0xd8, 0x6a, 0x7b, 0x04, 0x30, 0xd8, 0xcd, 0xb7, 0x80, 0x70, 0xb4, 0xc5, 0x5a]);
        assert_eq!(Aes::new(&key).decrypt(cipher), plain);
        let k = derive(b"peanuts", 1);
        for text in ["", "a", "sixteen bytes!!!", "{\"schemaVersion\":1,\"targets\":[]}"] {
            let sealed = encrypt_cbc(text.as_bytes(), &k);
            assert_eq!(sealed.len() % 16, 0);
            assert_eq!(decrypt_cbc(&sealed, &k, "", 0).as_deref(), Some(text));
        }
    }

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    fn key(s: &str) -> [u8; 16] {
        hex(s).try_into().unwrap()
    }

    #[test]
    fn aes_matches_the_fips_197_vector() {
        let aes = Aes::new(&key("000102030405060708090a0b0c0d0e0f"));
        let plain = aes.decrypt(key("69c4e0d86a7b0430d8cdb78070b4c55a"));
        assert_eq!(plain.to_vec(), hex("00112233445566778899aabbccddeeff"));
    }

    #[test]
    fn keys_derive_as_oscrypt_does() {
        assert_eq!(derive(b"peanuts", 1), key("fd621fe5a2b402539dfa147ca9272778"));
        assert_eq!(derive(b"secret", 1003), key("1a7404704ee35b4506624ed49171a534"));
        assert_eq!(derive(b"", 1), key("d0d0ec9c7d77d43ac54187fa4818d17f"));
    }

    #[test]
    fn values_decrypt_with_the_v10_key_and_strip_the_host_binding() {
        let linux = Keys { v10: Some(derive(b"peanuts", 1)), empty: Some(derive(b"", 1)), ..Keys::default() };
        assert_eq!(decrypt(&hex("763130ce9b849d984a1cedb8ba4eea782c2680"), &linux, "x", 23).as_deref(), Some("hello"));
        let mac = Keys { v10: Some(derive(b"secret", 1003)), ..Keys::default() };
        let bound = hex("76313089c2cd607ed07a706787462740b58d2b59724a2764c3b827b625e12bdabcbdd860b70e751be1589c67b82b4935c2a972");
        assert_eq!(decrypt(&bound, &mac, ".example.com", 24).as_deref(), Some("value"));
        // Another host's binding, or a key that pads wrong, is undecryptable.
        assert_eq!(decrypt(&bound, &mac, ".other.com", 24), None);
        assert_eq!(decrypt(&bound, &linux, ".example.com", 24), None);
        // v11 needs the keyring's key.
        assert_eq!(decrypt(b"v11aaaaaaaaaaaaaaaa", &linux, "x", 23), None);
    }

    #[test]
    fn a_chrome_cookie_database_reads_into_cdp_cookies() {
        // A schema-24 Chrome `Cookies` file with 512-byte pages, so its b-tree has interior pages
        // and one value spills onto overflow pages.
        let data = include_bytes!("testdata-cookies.sqlite");
        let keys = Keys { v10: Some(derive(b"peanuts", 1)), empty: Some(derive(b"", 1)), ..Keys::default() };
        let read = read_cookies(data, &keys).unwrap();
        assert_eq!(read.cookies.len(), 78);
        assert_eq!(read.skipped, 2);
        assert_eq!(read.domains, vec!["partitioned.test", "bad.test"]);
        let find = |name: &str| read.cookies.iter().find(|(c, _)| c["name"] == name).unwrap().0.clone();
        assert_eq!(
            find("bound"),
            json!({ "url": "https://example.com/", "domain": ".example.com", "name": "bound", "value": "value", "path": "/", "secure": true, "httpOnly": true, "sameSite": "Strict", "expires": 13_400_000_000i64 - 11_644_473_600 })
        );
        assert_eq!(find("plain"), json!({ "url": "http://app.test/x", "name": "plain", "value": "visible", "path": "/x", "secure": false, "httpOnly": false }));
        assert_eq!(find("big")["value"], json!("B".repeat(1500)));
        assert_eq!(find("n42")["sameSite"], json!("None"));
    }

    #[test]
    fn a_firefox_database_reads_with_its_write_ahead_log_and_only_the_default_container() {
        // A schema-16 `cookies.sqlite` in WAL mode: `late` and sid's new value are only in the log.
        let db = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/panel/testdata-firefox.sqlite"));
        let main_only = read_gecko(&std::fs::read(db).unwrap()).unwrap();
        assert_eq!(main_only.cookies.len(), 2, "the main file alone predates the log");
        let read = read_gecko(&gecko_snapshot(db).unwrap()).unwrap();
        let find = |name: &str| read.cookies.iter().find(|(c, _)| c["name"] == name).map(|c| c.0.clone());
        assert_eq!(read.cookies.len(), 3);
        assert_eq!(find("container"), None, "containers can't be kept apart in Chrome");
        assert_eq!(
            find("sid").unwrap(),
            json!({ "url": "https://example.com/", "domain": ".example.com", "name": "sid", "value": "abc2", "path": "/", "secure": true, "httpOnly": true, "sameSite": "None", "expires": 1_900_000_000i64 })
        );
        assert_eq!(find("lax").unwrap(), json!({ "url": "http://app.test/x", "name": "lax", "value": "l", "path": "/x", "secure": false, "httpOnly": false, "sameSite": "Lax" }));
        assert_eq!(find("late").unwrap()["sameSite"], json!("Strict"));
        assert_eq!(count(Engine::Firefox, db), Some(3));
    }

    #[test]
    fn firefox_same_site_keeps_an_undeclared_value_unset() {
        assert_eq!(gecko_same_site(&Val::Null, &Val::Null), None, "schema 9 rows carry NULL");
        assert_eq!(gecko_same_site(&Val::Int(1), &Val::Int(0)), None, "defaulted to Lax, declared nothing");
        assert_eq!(gecko_same_site(&Val::Int(1), &Val::Int(1)), Some("Lax"));
        assert_eq!(gecko_same_site(&Val::Int(1), &Val::Null), Some("Lax"));
        assert_eq!(gecko_same_site(&Val::Int(0), &Val::Null), Some("None"));
        assert_eq!(gecko_same_site(&Val::Int(2), &Val::Null), Some("Strict"));
        assert_eq!(gecko_same_site(&Val::Int(7), &Val::Null), None);
    }

    #[test]
    fn firefox_profiles_come_from_profile_blocks_and_stay_inside_the_root() {
        // An absolute path names a drive on Windows.
        let absolute = if cfg!(windows) { "C:\\data\\ff" } else { "/data/ff" };
        let ini = format!("[Install4F96D1932A9F858E]\nDefault=Profiles/x.default-release\n\n[Profile1]\nName=work\nIsRelative=1\nPath=Profiles/abc.work\n\n[Profile0]\nName= default-release \nIsRelative=1\nPath=Profiles/x.default-release\n\n[Profile2]\nName=escape\nIsRelative=1\nPath=../../etc\n\n[Profile3]\nName=custom\nIsRelative=0\nPath={absolute}\n\n[Profile4]\nIsRelative=0\nPath=relative/again\n\n[General]\nPath=ignored\n");
        assert_eq!(
            firefox_profiles(&ini),
            vec![
                ("Profiles/abc.work".to_owned(), "work".to_owned()),
                ("Profiles/x.default-release".to_owned(), "default-release".to_owned()),
                (absolute.to_owned(), "custom".to_owned()),
            ]
        );
    }

    #[test]
    fn a_firefox_lock_link_is_stale_only_for_a_dead_local_pid() {
        let local = vec!["127.0.1.1".to_owned()];
        assert!(gecko_link_held("127.0.1.1:+4242", &local, |_| true));
        assert!(!gecko_link_held("127.0.1.1:+4242", &local, |_| false));
        assert!(!gecko_link_held("127.0.1.1:4242", &local, |_| false));
        assert!(gecko_link_held("10.0.0.9:+4242", &local, |_| false), "another machine's lock");
        assert!(gecko_link_held("127.0.1.1:+42x", &local, |_| false));
        assert!(gecko_link_held("127.0.1.1:++42", &local, |_| false));
        assert!(gecko_link_held("garbage", &local, |_| false));
    }

    /// A `Cookies.binarycookies` file of one page holding `records` (flags, domain, name, path,
    /// value, Apple-epoch expiry).
    fn jar(records: &[(u32, &str, &str, &str, &str, f64)]) -> Vec<u8> {
        let bodies: Vec<Vec<u8>> = records
            .iter()
            .map(|(flags, domain, name, path, value, expiry)| {
                let mut strings = Vec::new();
                let mut offsets = Vec::new();
                for s in [domain, name, path, value] {
                    offsets.push(56 + strings.len() as u32);
                    strings.extend_from_slice(s.as_bytes());
                    strings.push(0);
                }
                let mut r = Vec::new();
                r.extend((56 + strings.len() as u32).to_le_bytes());
                r.extend(0u32.to_le_bytes());
                r.extend(flags.to_le_bytes());
                r.extend(0u32.to_le_bytes());
                for o in offsets {
                    r.extend(o.to_le_bytes());
                }
                r.extend([0u8; 8]);
                r.extend(expiry.to_le_bytes());
                r.extend(0f64.to_le_bytes());
                r.extend(strings);
                r
            })
            .collect();
        let mut page = vec![0, 0, 1, 0];
        page.extend((records.len() as u32).to_le_bytes());
        let mut at = 12 + 4 * records.len() as u32;
        for b in &bodies {
            page.extend(at.to_le_bytes());
            at += b.len() as u32;
        }
        page.extend(0u32.to_le_bytes());
        for b in bodies {
            page.extend(b);
        }
        let mut file = b"cook".to_vec();
        file.extend(1u32.to_be_bytes());
        file.extend((page.len() as u32).to_be_bytes());
        file.extend(page);
        file.extend([0u8; 8]);
        file
    }

    #[test]
    fn a_safari_jar_reads_its_records_and_rejects_a_malformed_file() {
        let data = jar(&[(5, ".example.com", "sid", "/", "v", 700_000_000.5), (0, "app.test", "plain", "", "x", 0.0), (0, "", "nodomain", "/", "x", 0.0)]);
        let cookies = parse_binary_cookies(&data).unwrap();
        assert_eq!(cookies.len(), 2, "a record without a domain is dropped");
        assert_eq!(
            cookies[0].0,
            json!({ "url": "https://example.com/", "domain": ".example.com", "name": "sid", "value": "v", "path": "/", "secure": true, "httpOnly": true, "sameSite": "Lax", "expires": 700_000_000i64 + APPLE_EPOCH_OFFSET })
        );
        assert_eq!(cookies[1].0["url"], json!("http://app.test/"), "an empty path is the root");
        assert_eq!(parse_binary_cookies(b"nope").err(), Some("readFailed"));
        assert_eq!(parse_binary_cookies(&data[..data.len() - 3]).err(), Some("readFailed"), "a truncated trailer");
        let mut overlapping = data.clone();
        // The page starts at byte 12 and its record offsets at 8 within it: point the second at the first.
        let first = u32::from_le_bytes(overlapping[20..24].try_into().unwrap());
        overlapping[24..28].copy_from_slice(&first.to_le_bytes());
        assert_eq!(parse_binary_cookies(&overlapping).err(), Some("readFailed"));
    }

    #[test]
    fn a_file_that_is_not_sqlite_fails_to_read() {
        assert_eq!(read_cookies(b"not sqlite", &Keys::default()).err(), Some("readFailed"));
    }

    #[test]
    fn the_singleton_lock_is_stale_only_for_a_dead_pid_on_this_host() {
        assert!(lock_held("mac.local-123", "mac.local", |_| true));
        assert!(!lock_held("mac.local-123", "mac.local", |_| false));
        assert!(lock_held("other-123", "mac.local", |_| false));
        assert!(lock_held("garbage", "mac.local", |_| false));
        assert!(lock_held("mac.local-12a", "mac.local", |_| false));
    }

    #[test]
    fn writes_finish_the_job_with_their_failures_counted() {
        let job: Job = Arc::new(Mutex::new(Stage::Writing { left: 2, imported: 0, skipped: 1, domains: vec!["a.test".into()] }));
        assert!(!written(&job, true, "b.test"));
        assert!(written(&job, false, "c.test"));
        assert_eq!(*job.lock().unwrap(), Stage::Done { imported: 1, skipped: 2, domains: vec!["a.test".into(), "c.test".into()] });
        assert_eq!(skipped_domains(&["a".into(), "b".into(), "c".into(), "d".into()]), "a, b, c and 1 more");
        assert_eq!(skipped_domains(&["a".into(), "b".into()]), "a and b");
    }
}
