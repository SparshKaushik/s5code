//! Where the app's server state comes from: a live `s5_net::Net`, or recorded fixtures for
//! headless scenes (`fixtures/`, dumped from a real server), plus small readers over the rows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use s5_net::{Conn, Net, State, ThreadView};
use serde_json::Value;

pub enum Source {
    Live(Net),
    Fixture(State),
}

impl Source {
    pub fn state(&self) -> &State {
        match self {
            Source::Live(net) => net.state(),
            Source::Fixture(state) => state,
        }
    }

    pub fn net(&mut self) -> Option<&mut Net> {
        match self {
            Source::Live(net) => Some(net),
            Source::Fixture(_) => None,
        }
    }
}

pub fn empty_state() -> State {
    State {
        conn: Conn::Connecting,
        config: None,
        projects: Vec::new(),
        threads: Vec::new(),
        archived: Vec::new(),
        shell_ready: false,
        shell_sequence: 0,
        thread_views: HashMap::new(),
        vcs: HashMap::new(),
    }
}

/// `S5_FIXTURES`, else `fixtures/` next to the crate (where `cargo run` and the harness start).
pub fn fixtures_dir() -> PathBuf {
    std::env::var_os("S5_FIXTURES").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures"))
}

fn read_json(path: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// The recorded server: shell snapshot, config, and every thread projection under `threads/`.
pub fn fixture_state() -> State {
    let dir = fixtures_dir();
    let mut state = empty_state();
    state.conn = Conn::Open;
    if let Some(shell) = read_json(&dir.join("shell.json")) {
        let rows = |k: &str| shell[k].as_array().cloned().unwrap_or_default();
        state.projects = rows("projects");
        state.threads = rows("threads");
        state.archived = rows("archivedThreads");
        state.shell_ready = true;
    }
    state.config = read_json(&dir.join("config.json"));
    // `vcs.json`: cwd -> `{local, remote}`, as the `subscribeVcsStatus` stream folds into `State::vcs`.
    if let Some(Value::Object(vcs)) = read_json(&dir.join("vcs.json")) {
        state.vcs = vcs.into_iter().collect();
    }
    if let Ok(entries) = std::fs::read_dir(dir.join("threads")) {
        for entry in entries.flatten() {
            if let Some(projection) = read_json(&entry.path()) {
                let id = projection["thread"]["id"].as_str().unwrap_or_default().to_string();
                state.thread_views.insert(id, ThreadView { projection, synchronized: true, sequence: 0 });
            }
        }
    }
    state
}

/// Reads a string field, `""` when absent or null.
pub fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}

/// An ISO-8601 UTC timestamp (`2026-11-06T09:47:25.123Z`) as Unix milliseconds.
pub fn iso_ms(iso: &str) -> Option<i64> {
    let b = iso.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| iso.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, sec) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let mut ms = 0;
    if b.get(19) == Some(&b'.') {
        let frac: String = iso[20..].chars().take_while(|c| c.is_ascii_digit()).collect();
        ms = format!("{frac:0<3}")[..3].parse().unwrap_or(0);
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * ((mo + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60_000 + sec * 1000 + ms)
}

pub fn ts(v: &Value, key: &str) -> Option<i64> {
    v[key].as_str().and_then(iso_ms)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Where wall-clock labels are read: the machine's time zone, as the web's local `Date`
/// getters, or a fixed offset in tests.
#[derive(Clone, Copy, Debug, Default)]
pub enum Zone {
    #[default]
    Local,
    Fixed(i64),
}

impl Zone {
    /// `at` as wall-clock milliseconds: whole days start at local midnight.
    pub fn wall(self, at: i64) -> i64 {
        at + self.offset(at)
    }

    /// The instant a wall-clock time names, using the offset in force then (across DST too).
    pub fn instant(self, wall: i64) -> i64 {
        wall - self.offset(wall - self.offset(wall))
    }

    fn offset(self, at: i64) -> i64 {
        match self {
            Zone::Fixed(offset) => offset,
            Zone::Local => local_offset_ms(at),
        }
    }
}

#[cfg(unix)]
fn local_offset_ms(at: i64) -> i64 {
    // `struct tm` on glibc and macOS: nine ints, then `tm_gmtoff` and `tm_zone`.
    #[repr(C)]
    struct Tm {
        ints: [std::ffi::c_int; 9],
        gmtoff: std::ffi::c_long,
        zone: *const std::ffi::c_char,
    }
    unsafe extern "C" {
        fn tzset();
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }
    static TZSET: std::sync::Once = std::sync::Once::new();
    TZSET.call_once(|| unsafe { tzset() });
    let secs = at.div_euclid(1000);
    let mut tm = Tm { ints: [0; 9], gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: both pointers are valid for the call; `localtime_r` writes only `tm`.
    let ok = unsafe { !localtime_r(&secs, &mut tm).is_null() };
    if ok { tm.gmtoff as i64 * 1000 } else { 0 }
}

#[cfg(not(unix))]
fn local_offset_ms(_at: i64) -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::iso_ms;

    #[test]
    fn parses_iso() {
        assert_eq!(iso_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(iso_ms("2026-11-06T09:47:25.5Z"), Some(1_793_958_445_500));
        assert_eq!(iso_ms("2000-02-29T12:00:00Z"), Some(951_825_600_000));
    }
}
