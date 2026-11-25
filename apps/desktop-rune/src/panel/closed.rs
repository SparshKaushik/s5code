//! Recently closed views for `view.reopenClosed` (the web's `closedViewStore` and
//! `ReopenClosedViewShortcut`): thread panel views and the pull requests page's tabs share one
//! list. Terminals are never recorded; newest first, at most 20. Like the web's persisted store,
//! the list outlives a restart.

use serde_json::{Value, json};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum View {
    Files(String),
    Attachment(String),
    Device { host: String, device: String, name: String, title: Option<String> },
    Diff,
    Browser(String),
    /// A side chat closed beside its thread (the web's `side-chat:` tab): the side chat's id.
    SideChat(String),
    PullRequests,
    /// A pull requests page tab, as the entry it showed.
    PullRequest(Value),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Closed {
    pub thread: String,
    pub view: View,
}

impl Closed {
    /// The web's `sameTarget`: a pull request tab is the same view while its URL is.
    fn same_target(&self, other: &Closed) -> bool {
        self.thread == other.thread && self.view.same_target(&other.view)
    }
}

impl View {
    pub(super) fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::PullRequest(a), Self::PullRequest(b)) => a["url"] == b["url"],
            (Self::Device { host: a, device: b, .. }, Self::Device { host: c, device: d, .. }) => a == c && b == d,
            _ => self == other,
        }
    }
}

/// The web's `PULL_REQUESTS_PANEL_REF`: the owner of views closed on the pull requests page.
pub const PAGE: &str = "pull-requests-panel";

const CAPACITY: usize = 20;
const FILE: &str = "closed-views.json";
/// The pull requests page's own list before the two merged.
const PAGE_FILE: &str = "pull-request-closed-views.json";

/// A pull request entry worth reopening (`isClosedViewEntry`'s pull-request case).
pub(super) fn pull_request(e: &Value) -> Option<Value> {
    (e["url"].as_str().is_some_and(|u| !u.is_empty()) && e["number"].as_u64().is_some_and(|n| n > 0)).then(|| e.clone())
}

/// The old page list's tabs join after the shared list's views, which they cannot be ordered against.
fn migrate(mut entries: Vec<Closed>, page: &Value) -> Vec<Closed> {
    for e in page["entries"].as_array().into_iter().flatten().filter_map(pull_request) {
        let c = Closed { thread: PAGE.into(), view: View::PullRequest(e) };
        if !entries.iter().any(|x| x.same_target(&c)) {
            entries.push(c);
        }
    }
    entries.truncate(CAPACITY);
    entries
}

fn read(path: &std::path::Path) -> Option<Value> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok())
}

#[derive(Default)]
pub struct Store {
    entries: Vec<Closed>,
    loaded: bool,
}

fn encode(entries: &[Closed]) -> Value {
    let rows: Vec<Value> = entries
        .iter()
        .map(|c| match &c.view {
            View::Attachment(id) => json!({"thread":c.thread, "kind":"attachment", "id":id}),
            View::Device { host, device, name, title } => json!({"thread":c.thread, "kind":"device", "host":host, "device":device, "name":name, "title":title}),
            View::Files(path) => json!({"thread": c.thread, "kind": "files", "path": path}),
            View::Diff => json!({"thread": c.thread, "kind": "diff"}),
            View::PullRequests => json!({"thread": c.thread, "kind": "pull-requests"}),
            View::Browser(url) => json!({"thread": c.thread, "kind": "browser", "url": url}),
            View::SideChat(side) => json!({"thread": c.thread, "kind": "side-chat", "threadId": side}),
            View::PullRequest(entry) => json!({"thread": c.thread, "kind": "pull-request", "entry": entry}),
        })
        .collect();
    json!({"version": 1, "entries": rows})
}

/// Unknown or malformed rows drop, as the web's `migrate` filters with `isClosedViewEntry`.
fn decode(v: &Value) -> Vec<Closed> {
    v["entries"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let thread = row["thread"].as_str().filter(|t| !t.is_empty())?.to_owned();
            let view = match row["kind"].as_str()? {
                "attachment" | "device" => super::tabs::Target::decode(row)?.closed()?,
                "files" => View::Files(row["path"].as_str()?.to_owned()),
                "diff" => View::Diff,
                "pull-requests" => View::PullRequests,
                "browser" => View::Browser(row["url"].as_str()?.to_owned()),
                "side-chat" => View::SideChat(row["threadId"].as_str().filter(|t| !t.is_empty())?.to_owned()),
                "pull-request" => View::PullRequest(pull_request(&row["entry"])?),
                _ => return None,
            };
            Some(Closed { thread, view })
        })
        .take(CAPACITY)
        .collect()
}

impl Store {
    fn path(file: &str) -> Option<std::path::PathBuf> {
        (!cfg!(test)).then(|| crate::settings::client::sibling_path(file))
    }

    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        if let Some(saved) = Self::path(FILE).and_then(|p| read(&p)) {
            self.entries = decode(&saved);
        }
        if let Some(old) = Self::path(PAGE_FILE)
            && let Some(page) = read(&old)
        {
            self.entries = migrate(std::mem::take(&mut self.entries), &page);
            if self.save() {
                let _ = std::fs::remove_file(old);
            }
        }
    }

    fn save(&self) -> bool {
        let Some(path) = Self::path(FILE) else { return false };
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let written = serde_json::to_vec(&encode(&self.entries)).ok().is_some_and(|bytes| {
            path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, &path).is_ok()
        });
        if !written {
            eprintln!("panel: saving closed views failed");
        }
        written
    }

    /// A view that reopens to the same target replaces its older entry.
    pub fn record(&mut self, closed: Closed) {
        self.remember([closed]);
    }

    /// `remember` for views closed together, in order: the last reopens first.
    pub fn remember(&mut self, views: impl IntoIterator<Item = Closed>) {
        self.load();
        for closed in views.into_iter().filter(|c| !c.thread.is_empty()) {
            self.entries.retain(|c| !c.same_target(&closed));
            self.entries.insert(0, closed);
        }
        self.entries.truncate(CAPACITY);
        self.save();
    }

    pub fn is_empty(&mut self) -> bool {
        self.load();
        self.entries.is_empty()
    }

    /// `planNextReopen`: drops entries whose thread is gone or that are open already, and
    /// takes the newest that is left.
    pub fn next(&mut self, exists: impl Fn(&str) -> bool, open: impl Fn(&Closed) -> bool) -> Option<Closed> {
        self.load();
        let mut found = None;
        while !self.entries.is_empty() {
            let c = self.entries.remove(0);
            if exists(&c.thread) && !open(&c) {
                found = Some(c);
                break;
            }
        }
        self.save();
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn closed(thread: &str, view: View) -> Closed {
        Closed { thread: thread.into(), view }
    }

    #[test]
    fn reopens_newest_first_skipping_gone_and_open_views() {
        let mut store = Store::default();
        store.record(closed("a", View::Diff));
        store.record(closed("a", View::Files("src/lib.rs".into())));
        store.record(closed("gone", View::Diff));
        store.record(closed("a", View::Browser("http://localhost:3000/".into())));
        store.record(closed("a", View::Diff));
        store.record(closed("", View::Diff));
        let exists = |t: &str| t != "gone";
        assert_eq!(store.next(exists, |_| false), Some(closed("a", View::Diff)), "a re-close moves to the front");
        assert_eq!(store.next(exists, |_| false), Some(closed("a", View::Browser("http://localhost:3000/".into()))));
        assert_eq!(store.next(exists, |c| c.view == View::Files("src/lib.rs".into())), None, "gone and already-open entries drop");
        assert!(store.is_empty());

        for i in 0..25 {
            store.record(closed("a", View::Files(i.to_string())));
        }
        assert_eq!(store.entries.len(), 20);
        assert_eq!(store.entries[0].view, View::Files("24".into()));
    }

    #[test]
    fn saved_views_read_back_and_unknown_rows_drop() {
        let entries = vec![
            closed("a", View::Files("src/lib.rs".into())),
            closed("b", View::Diff),
            closed("c", View::Browser("http://localhost:5173/".into())),
            closed("d", View::SideChat("side-1".into())),
            closed("e", View::PullRequests),
            closed("a", View::Attachment("attachment-1".into())),
            closed("a", View::Device { host: "host".into(), device: "phone".into(), name: "Phone".into(), title: Some("My phone".into()) }),
        ];
        assert_eq!(decode(&encode(&entries)), entries);
        let saved = json!({"entries": [
            {"thread": "a", "kind": "terminal"},
            {"thread": "", "kind": "diff"},
            {"thread": "b", "kind": "files"},
            {"thread": "c", "kind": "diff"},
        ]});
        assert_eq!(decode(&saved), vec![closed("c", View::Diff)]);
        assert!(decode(&json!(null)).is_empty());
    }

    fn pr(n: u64, state: &str) -> Value {
        json!({"url": format!("https://github.com/acme/app/pull/{n}"), "number": n, "state": state, "env": "remote"})
    }

    #[test]
    fn page_tabs_and_thread_views_share_one_newest_first_list() {
        let mut store = Store::default();
        store.record(closed("a", View::Diff));
        store.remember([closed(PAGE, View::PullRequest(pr(1, "open"))), closed(PAGE, View::PullRequest(pr(2, "open")))]);
        store.record(closed("a", View::PullRequests));
        store.record(closed(PAGE, View::PullRequest(pr(1, "merged"))));
        assert_eq!(store.entries.len(), 4, "a pull request closed again replaces its older entry");
        let exists = |t: &str| t == PAGE || t == "a";
        assert_eq!(store.next(exists, |_| false), Some(closed(PAGE, View::PullRequest(pr(1, "merged")))), "the newest wins, whichever surface closed it");
        assert_eq!(store.next(exists, |_| false), Some(closed("a", View::PullRequests)));
        assert_eq!(store.next(exists, |c| c.view == View::PullRequest(pr(2, "open"))), Some(closed("a", View::Diff)), "a tab already showing drops");
        assert!(store.is_empty());

        store.remember((1..=25).map(|n| closed(PAGE, View::PullRequest(pr(n, "open")))));
        assert_eq!(store.entries.len(), CAPACITY);
        assert_eq!(store.entries[0].view, View::PullRequest(pr(25, "open")), "of tabs closed together the last reopens first");
    }

    #[test]
    fn the_page_list_migrates_after_the_shared_views_and_page_rows_read_back() {
        let shared = vec![closed("a", View::Diff), closed(PAGE, View::PullRequest(pr(1, "open")))];
        let page = json!({"version": 1, "entries": [pr(1, "closed"), {"url": "", "number": 3}, {"url": "https://github.com/acme/app/pull/4"}, pr(5, "open")]});
        let merged = migrate(shared.clone(), &page);
        assert_eq!(merged, vec![shared[0].clone(), shared[1].clone(), closed(PAGE, View::PullRequest(pr(5, "open")))]);
        assert_eq!(decode(&encode(&merged)), merged, "the entry and its environment tag survive a restart");
        let rows: Vec<Value> = (1..=25).map(|n| pr(n, "open")).collect();
        let many = json!({"entries": rows});
        assert_eq!(migrate(vec![closed("a", View::Diff)], &many).len(), CAPACITY);
        assert!(decode(&json!({"entries": [{"thread": PAGE, "kind": "pull-request", "entry": {"url": "x", "number": 0}}]})).is_empty());
    }
}
