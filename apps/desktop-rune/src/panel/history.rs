//! Web's `browserHistoryStore`: each project's visited URLs, saved in `browser-history.json`.
use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::{App, Route};

const MAX_ENTRIES: usize = 50;
const MAX_PROJECTS: usize = 20;
const MAX_URL: usize = 2048;
const MAX_TITLE: usize = 512;
const FILE: &str = "browser-history.json";

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub url: String,
    pub at: i64,
    pub title: String,
}

#[derive(Default)]
pub struct History {
    projects: HashMap<String, Vec<Entry>>,
    loaded: bool,
}

/// A URL's `(scheme, authority, path, query and fragment)`.
fn split(url: &str) -> Option<(&str, &str, &str, &str)> {
    let (scheme, rest) = url.split_once("://")?;
    let a = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let p = rest[a..].find(['?', '#']).map_or(rest.len(), |i| a + i);
    Some((scheme, &rest[..a], &rest[a..p], &rest[p..]))
}

/// An authority's host and port, past any credentials.
fn host_port(authority: &str) -> (&str, &str) {
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    match authority.rfind(':').filter(|&i| !authority[i..].contains(']')) {
        Some(i) => (&authority[..i], &authority[i + 1..]),
        None => (authority, ""),
    }
}

/// `normalizeHistoryUrl`: an http(s) URL as `URL.href` spells it, without credentials.
pub fn normalize(raw: &str) -> Option<String> {
    let (scheme, authority, path, tail) = split(raw.trim())?;
    let scheme = scheme.to_ascii_lowercase();
    let default = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return None,
    };
    let (host, port) = host_port(authority);
    let port = match port {
        "" => None,
        p => Some(p.parse::<u16>().ok()?),
    };
    if host.is_empty() {
        return None;
    }
    let port = port.filter(|&p| p != default).map_or(String::new(), |p| format!(":{p}"));
    let path = if path.is_empty() { "/" } else { path };
    let href = format!("{scheme}://{}{port}{path}{tail}", host.to_ascii_lowercase());
    (href.len() <= MAX_URL).then_some(href)
}

/// `isLocalLoopbackHost` plus `0.0.0.0`: the hosts one dev server answers on interchangeably.
fn stable_local(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']').trim_end_matches('.');
    let ipv4: Vec<_> = host.split('.').map(|p| p.parse::<u8>()).collect();
    matches!(host, "localhost" | "::1" | "0.0.0.0") || ipv4.len() == 4 && ipv4.iter().all(|p| p.is_ok()) && host.starts_with("127.")
}

fn url_is_stable_local(url: &str) -> bool {
    split(url).is_some_and(|(_, a, _, _)| stable_local(host_port(a).0))
}

/// `visitLookupKey`: loopback spellings of one server are one entry.
fn visit_key(url: &str) -> String {
    match split(url) {
        Some((scheme, authority, path, tail)) => {
            let (host, port) = host_port(authority);
            let host = if stable_local(host) { "local" } else { host };
            let port = if port.is_empty() { String::new() } else { format!(":{port}") };
            format!("{scheme}://{host}{port}{path}{tail}")
        }
        None => url.to_string(),
    }
}

/// A page's origin, loopback spellings as one (web's favicon key).
pub(super) fn origin_key(url: &str) -> Option<String> {
    let normalized = normalize(url)?;
    let (scheme, authority, _, _) = split(&normalized)?;
    let (host, port) = host_port(authority);
    let host = if stable_local(host) { "local" } else { host };
    Some(if port.is_empty() { format!("{scheme}://{host}") } else { format!("{scheme}://{host}:{port}") })
}

/// `PreviewRecentUrlCard`'s label: the host, then the path unless it is `/`, query and hash.
pub(super) fn label(url: &str) -> String {
    match split(url) {
        Some((_, authority, path, tail)) => {
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            format!("{host}{}{tail}", if path == "/" { "" } else { path })
        }
        None => url.to_string(),
    }
}

/// `titleLookupKey`: a page's title also names its trailing-slash twin.
fn title_key(url: &str) -> String {
    let key = visit_key(url);
    match split(&key) {
        Some((scheme, authority, path, tail)) if path.len() > 1 && path.ends_with('/') => {
            format!("{scheme}://{authority}{}{tail}", &path[..path.len() - 1])
        }
        _ => key,
    }
}

/// `upsertHistoryEntry`: the visit moves to the front, keeping the stable local spelling.
fn upsert(entries: &mut Vec<Entry>, url: String, at: i64) {
    let key = visit_key(&url);
    let existing = entries.iter().position(|e| visit_key(&e.url) == key).map(|i| entries.remove(i));
    let entry = match existing {
        Some(e) => Entry { url: if url_is_stable_local(&e.url) || !url_is_stable_local(&url) { e.url } else { url }, at, title: e.title },
        None => Entry { url, at, title: String::new() },
    };
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
}

/// `evictExcessProjects`: the projects with the most recent visits stay.
fn evict(projects: &mut HashMap<String, Vec<Entry>>) {
    if projects.len() <= MAX_PROJECTS {
        return;
    }
    let mut keys: Vec<(i64, String)> = projects.iter().map(|(k, v)| (v.first().map_or(0, |e| e.at), k.clone())).collect();
    keys.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, key) in keys.into_iter().skip(MAX_PROJECTS) {
        projects.remove(&key);
    }
}

impl History {
    fn from_json(v: &Value) -> Self {
        let mut projects = HashMap::new();
        for (key, rows) in v["byProjectKey"].as_object().into_iter().flatten() {
            let mut entries: Vec<Entry> = rows
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|r| {
                    let url = normalize(r["url"].as_str()?)?;
                    let title: String = r["title"].as_str().unwrap_or("").chars().take(MAX_TITLE).collect();
                    Some(Entry { url, at: r["lastVisitedAt"].as_i64()?, title })
                })
                .collect();
            entries.sort_by(|a, b| b.at.cmp(&a.at));
            let mut seen = std::collections::HashSet::new();
            entries.retain(|e| seen.insert(visit_key(&e.url)));
            entries.truncate(MAX_ENTRIES);
            if !entries.is_empty() {
                projects.insert(key.clone(), entries);
            }
        }
        evict(&mut projects);
        Self { projects, loaded: true }
    }

    fn to_json(&self) -> Value {
        let mut by = Map::new();
        for (key, entries) in &self.projects {
            let rows: Vec<Value> = entries
                .iter()
                .map(|e| if e.title.is_empty() { json!({"url": e.url, "lastVisitedAt": e.at}) } else { json!({"url": e.url, "lastVisitedAt": e.at, "title": e.title}) })
                .collect();
            by.insert(key.clone(), Value::Array(rows));
        }
        json!({"version": 1, "byProjectKey": by})
    }

    fn record(&mut self, project: &str, url: &str, at: i64) -> bool {
        let Some(url) = normalize(url) else { return false };
        upsert(self.projects.entry(project.to_string()).or_default(), url, at);
        evict(&mut self.projects);
        true
    }

    fn set_title(&mut self, project: &str, url: &str, title: &str) -> bool {
        let title: String = title.trim().chars().take(MAX_TITLE).collect();
        let (Some(url), Some(entries)) = (normalize(url), self.projects.get_mut(project)) else { return false };
        let key = title_key(&url);
        match entries.iter_mut().find(|e| title_key(&e.url) == key) {
            Some(e) if !title.is_empty() && e.title != title => {
                e.title = title;
                true
            }
            _ => false,
        }
    }

    fn remove(&mut self, project: &str, url: &str) -> bool {
        let Some(entries) = self.projects.get_mut(project) else { return false };
        let before = entries.len();
        entries.retain(|e| e.url != url);
        let changed = entries.len() != before;
        if entries.is_empty() {
            self.projects.remove(project);
        }
        changed
    }
}

/// History follows the thread's project; a new-thread draft uses the project it will start in.
fn project(app: &App) -> Option<String> {
    let id = match &app.route {
        Route::Thread(id) => app.state().threads.iter().find(|v| v["id"] == *id)?["projectId"].as_str()?.to_string(),
        Route::Home { .. } => crate::chat::home::draft_project(app)?["id"].as_str()?.to_string(),
        _ => return None,
    };
    Some(id)
}

fn load(app: &mut App) -> &mut History {
    if !app.ui.panel.history.loaded {
        let saved = if cfg!(test) { None } else { std::fs::read(app.client_settings.sibling(FILE)).ok() };
        app.ui.panel.history = saved.and_then(|b| serde_json::from_slice::<Value>(&b).ok()).map_or_else(
            || History { loaded: true, ..Default::default() },
            |v| History::from_json(&v),
        );
    }
    &mut app.ui.panel.history
}

fn save(app: &App) {
    if !cfg!(test) {
        super::write_state(&app.client_settings.sibling(FILE), &app.ui.panel.history.to_json());
    }
}

fn change(app: &mut App, f: impl FnOnce(&mut History, &str) -> bool) {
    let Some(project) = project(app) else { return };
    if f(load(app), &project) {
        save(app);
    }
}

/// `recordVisitForThread`: an address the user opened.
pub fn record(app: &mut App, url: &str) {
    let now = crate::data::now_ms();
    change(app, |h, p| h.record(p, url, now));
}

/// A settled page names its history entry, if it has one.
pub fn set_title(app: &mut App, url: &str, title: &str) {
    change(app, |h, p| h.set_title(p, url, title));
}

pub fn remove(app: &mut App, url: &str) {
    change(app, |h, p| h.remove(p, url));
}

/// Reads the saved file once, before the empty state first lists it.
pub fn ensure_loaded(app: &mut App) {
    load(app);
}

/// The current project's visits, newest first.
pub fn entries(app: &App) -> &[Entry] {
    project(app).and_then(|p| app.ui.panel.history.projects.get(&p)).map(|v| v.as_slice()).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_spells_urls_as_web_href() {
        for (raw, want) in [
            ("http://localhost:5173", Some("http://localhost:5173/")),
            ("HTTPS://User:Pw@Example.COM:443/a?b#c", Some("https://example.com/a?b#c")),
            ("http://example.com:80?q", Some("http://example.com/?q")),
            ("http://[::1]:3000/x", Some("http://[::1]:3000/x")),
            ("file:///etc/passwd", None),
            ("http://:80/", None),
        ] {
            assert_eq!(normalize(raw).as_deref(), want, "{raw}");
        }
        assert_eq!(normalize(&format!("https://e.com/{}", "a".repeat(MAX_URL))), None);
        assert_eq!(origin_key("http://127.0.0.1:5173/a?b").as_deref(), Some("http://local:5173"));
        assert_eq!(origin_key("https://Example.com:443/x").as_deref(), Some("https://example.com"));
        assert_eq!(label("http://localhost:5173/"), "localhost:5173");
        assert_eq!(label("https://e.com/docs/?q#h"), "e.com/docs/?q#h");
    }

    #[test]
    fn visits_dedupe_loopback_and_keep_the_stable_spelling() {
        let mut h = History::default();
        assert!(h.record("p", "http://localhost:5173/", 1));
        h.record("p", "https://example.com/", 2);
        h.record("p", "http://127.0.0.1:5173/", 3);
        let urls: Vec<_> = h.projects["p"].iter().map(|e| (e.url.as_str(), e.at)).collect();
        assert_eq!(urls, [("http://localhost:5173/", 3), ("https://example.com/", 2)]);
        h.record("p", "http://192.168.1.4:5173/", 4);
        assert_eq!(h.projects["p"].len(), 3, "a LAN address is another server");

        assert!(h.set_title("p", "http://0.0.0.0:5173", " Dev "), "titles follow loopback and slash twins");
        assert_eq!(h.projects["p"][1].title, "Dev");
        assert!(!h.set_title("p", "https://unvisited.dev/", "X"), "only visited pages take titles");
        assert!(!h.set_title("p", "http://localhost:5173/", "Dev"));
        assert!(h.remove("p", "https://example.com/"));
        assert!(h.remove("p", "http://localhost:5173/") && h.remove("p", "http://192.168.1.4:5173/"));
        assert!(!h.projects.contains_key("p"), "an emptied project is dropped");
    }

    #[test]
    fn history_caps_entries_and_projects_and_round_trips() {
        let mut h = History::default();
        for i in 0..60 {
            h.record("p", &format!("https://e.com/{i}"), i);
        }
        assert_eq!(h.projects["p"].len(), MAX_ENTRIES);
        assert_eq!(h.projects["p"][0].url, "https://e.com/59");
        for i in 0..MAX_PROJECTS as i64 {
            h.record(&format!("q{i}"), "https://e.com/", 100 + i);
        }
        assert_eq!(h.projects.len(), MAX_PROJECTS);
        assert!(!h.projects.contains_key("p"), "the stalest project goes");
        h.set_title("q3", "https://e.com/", "Title");
        let back = History::from_json(&h.to_json());
        assert_eq!(back.projects, h.projects);
    }
}
