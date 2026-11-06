//! Startup wiring for a double-clicked app: find or start `opencode serve`, and pick the projects
//! to show from opencode's own project list. A Finder launch has a bare PATH and `/` as its cwd.

use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

pub fn opencode_url() -> String {
    std::env::var("S5_OPENCODE_URL").unwrap_or_else(|_| "http://127.0.0.1:4096".into())
}

fn reachable(base: &str) -> bool {
    ureq::get(&format!("{base}/config/providers")).timeout(Duration::from_secs(2)).call().is_ok()
}

fn opencode_bin() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut candidates: Vec<String> = std::env::var("PATH").unwrap_or_default().split(':').map(|d| format!("{d}/opencode")).collect();
    for d in [".opencode/bin", ".bun/bin", ".local/bin", ".npm-global/bin"] {
        candidates.push(format!("{home}/{d}/opencode"));
    }
    candidates.extend(["/opt/homebrew/bin/opencode".into(), "/usr/local/bin/opencode".into()]);
    candidates.into_iter().find(|p| Path::new(p).is_file())
}

/// Starts `opencode serve` on the configured port when nothing answers there. The returned child
/// is ours to stop on quit; an already-running server is left alone.
pub fn ensure_opencode(base: &str) -> Option<Child> {
    if reachable(base) {
        return None;
    }
    let port = base.rsplit(':').next().and_then(|p| p.trim_end_matches('/').parse::<u16>().ok()).unwrap_or(4096);
    let bin = opencode_bin()?;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let mut child = Command::new(bin)
        .args(["serve", "--hostname", "127.0.0.1", "--port", &port.to_string()])
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(20) {
        if reachable(base) {
            return Some(child);
        }
        if let Ok(Some(_)) = child.try_wait() {
            return None;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Some(child)
}

/// `S5_PROJECTS=name=/dir,...` wins; otherwise opencode's most recently used projects, falling back
/// to the launch directory (or home when launched from Finder).
pub fn projects(base: &str) -> Vec<(String, String)> {
    if let Ok(spec) = std::env::var("S5_PROJECTS") {
        return spec.split(',').filter_map(|p| p.split_once('=')).map(|(n, d)| (n.into(), d.into())).collect();
    }
    let mut found: Vec<(u64, String)> = ureq::get(&format!("{base}/project"))
        .timeout(Duration::from_secs(3))
        .call()
        .ok()
        .and_then(|r| r.into_json::<Value>().ok())
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|p| {
            let dir = p["worktree"].as_str()?;
            (dir != "/" && Path::new(dir).is_dir()).then(|| (p["time"]["updated"].as_u64().unwrap_or(0), dir.to_string()))
        })
        .collect();
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.dedup_by(|a, b| a.1 == b.1);
    let mut dirs: Vec<String> = found.into_iter().take(8).map(|(_, d)| d).collect();
    if dirs.is_empty() {
        let cwd = std::env::current_dir().ok().map(|d| d.display().to_string()).filter(|d| d != "/");
        dirs.push(cwd.unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "/".into())));
    }
    dirs.into_iter().map(|d| (Path::new(&d).file_name().map_or(d.clone(), |n| n.to_string_lossy().into_owned()), d)).collect()
}
