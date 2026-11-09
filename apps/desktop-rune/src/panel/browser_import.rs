//! Browser import: reads a Chromium-family browser's cookies on this machine and writes them into
//! Rune's Chrome over CDP, as the Electron app's `BrowserImport` does into its session.
//!
//! The cookie database is read into memory (never opened for writing) and parsed here, since Rune
//! links no SQLite. Values use OSCrypt's AES-128-CBC: macOS keys come from the browser's Keychain
//! item (`security`), Linux keys from the fixed "peanuts" passphrase (v10) or libsecret
//! (`secret-tool`, v11). Safari and Firefox are not read.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, OnceLock};

use rune::Waker;
use serde_json::{Value, json};

use super::browser::Cmd;

pub struct Source {
    pub name: &'static str,
    service: &'static str,
    account: &'static str,
    mac: &'static [&'static str],
    /// Under `~/.config`; empty where the browser has no Linux build.
    linux: &'static [&'static str],
    /// The libsecret `application` attribute holding the v11 key.
    secret: &'static str,
}

pub const SOURCES: &[Source] = &[
    Source { name: "Chrome", service: "Chrome Safe Storage", account: "Chrome", mac: &["Google", "Chrome"], linux: &["google-chrome"], secret: "chrome" },
    Source { name: "Microsoft Edge", service: "Microsoft Edge Safe Storage", account: "Microsoft Edge", mac: &["Microsoft Edge"], linux: &["microsoft-edge"], secret: "msedge" },
    Source { name: "Brave", service: "Brave Safe Storage", account: "Brave", mac: &["BraveSoftware", "Brave-Browser"], linux: &["BraveSoftware", "Brave-Browser"], secret: "brave" },
    // Vivaldi keeps its Linux key under Chrome's application name, Helium under Chromium's.
    Source { name: "Vivaldi", service: "Vivaldi Safe Storage", account: "Vivaldi", mac: &["Vivaldi"], linux: &["vivaldi"], secret: "chrome" },
    Source { name: "Opera", service: "Opera Safe Storage", account: "Opera", mac: &["com.operasoftware.Opera"], linux: &["opera"], secret: "opera" },
    Source { name: "Arc", service: "Arc Safe Storage", account: "Arc", mac: &["Arc", "User Data"], linux: &[], secret: "" },
    Source { name: "Helium", service: "Helium Storage Key", account: "Helium", mac: &["net.imput.helium"], linux: &["net.imput.helium"], secret: "chromium" },
];

/// `BROWSER_IMPORT_FAILURE_COPY` for the reasons a Chromium import can fail with.
pub fn failure_copy(reason: &str) -> &'static str {
    match reason {
        "notInstalled" => "Not installed on this machine.",
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
    let (base, segments) = if cfg!(target_os = "macos") {
        (home.join("Library").join("Application Support"), source.mac)
    } else if cfg!(target_os = "linux") && !source.linux.is_empty() {
        (home.join(".config"), source.linux)
    } else {
        return None;
    };
    Some(segments.iter().fold(base, |p, s| p.join(s)))
}

/// The browsers installed here, in menu order.
pub fn installed() -> Vec<&'static Source> {
    SOURCES.iter().filter(|s| root(s).is_some_and(|r| r.is_dir())).collect()
}

fn safe(directory: &str) -> bool {
    !directory.is_empty() && directory != "." && directory != ".." && !directory.contains(['/', '\\'])
}

fn cookie_db(profile: &Path) -> Option<PathBuf> {
    [profile.join("Network").join("Cookies"), profile.join("Cookies")].into_iter().find(|p| p.is_file())
}

/// Profiles from `Local State`, or every directory holding a cookie database.
pub fn profiles(source: &Source) -> Vec<Profile> {
    let Some(root) = root(source) else { return Vec::new() };
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
            let db = cookie_db(&root.join(&directory))?;
            let cookies = std::fs::read(db).ok().and_then(|data| Some(Db::open(&data)?.table("cookies")?.rows.len()));
            Some(Profile { directory, name, cookies })
        })
        .collect()
}

/// Chrome's `SingletonLock` links to `host-pid` while a browser holds the profile.
pub fn running(source: &Source) -> bool {
    let Some(target) = root(source).and_then(|r| std::fs::read_link(r.join("SingletonLock")).ok()) else { return false };
    lock_held(&target.to_string_lossy(), &hostname(), alive)
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

fn derive(passphrase: &[u8], iterations: u32) -> [u8; 16] {
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
    let out = Command::new("security").args(["find-generic-password", "-w", "-s", service, "-a", account]).output().map_err(|_| "keychainUnavailable")?;
    match out.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&out.stdout).trim_end_matches('\n').to_owned()).filter(|s| !s.is_empty()).ok_or("keychainItemMissing"),
        // errSecItemNotFound
        Some(44) => Err("keychainItemMissing"),
        _ => Err("needsKeychainApproval"),
    }
}

fn linux_secret(application: &str) -> Result<String, &'static str> {
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

fn decrypt_cbc(payload: &[u8], key: &[u8; 16], host: &str, version: i64) -> Option<String> {
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

/// AES-128 decryption (FIPS-197 inverse cipher), the one primitive `ring` does not expose.
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

/// A CDP `CookieParam`, scoped as Electron's `cookieScope` does.
fn cookie_param(host: &str, name: &str, value: &str, path: &str, expires_utc: i64, secure: bool, http_only: bool, same_site: i64) -> Value {
    let bare_host = bare(host);
    let authority = if bare_host.contains(':') && !(bare_host.starts_with('[') && bare_host.ends_with(']')) { format!("[{bare_host}]") } else { bare_host.to_owned() };
    let scheme = if secure { "https" } else { "http" };
    let mut cookie = json!({ "url": format!("{scheme}://{authority}{path}"), "name": name, "value": value, "path": path, "secure": secure, "httpOnly": http_only });
    if host.starts_with('.') {
        cookie["domain"] = json!(host);
    }
    // Chrome stores expiry in microseconds since 1601.
    let seconds = expires_utc / 1_000_000;
    if seconds > 0 {
        cookie["expires"] = json!(seconds - 11_644_473_600);
    }
    if let Some(same_site) = match same_site {
        0 => Some("None"),
        1 => Some("Lax"),
        2 => Some("Strict"),
        _ => None,
    } {
        cookie["sameSite"] = json!(same_site);
    }
    cookie
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
        let cookie = cookie_param(&host, &col("name").text(), &value, &col("path").text(), col("expires_utc").int(), col("is_secure").int() == 1, col("is_httponly").int() == 1, col("samesite").int());
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
            let root = root(source).ok_or("unsupportedPlatform")?;
            let db = Some(directory.as_str()).filter(|d| safe(d)).and_then(|d| cookie_db(&root.join(d))).ok_or("unknownSourceProfile")?;
            let keys = keys(source)?;
            let data = std::fs::read(db).map_err(|_| "readFailed")?;
            read_cookies(&data, &keys)
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
