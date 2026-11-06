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
