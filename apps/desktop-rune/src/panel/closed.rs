//! Recently closed panel views for `view.reopenClosed` (the web's `closedViewStore` and
//! `ReopenClosedViewShortcut`). Terminals are never recorded; newest first, at most 20. Like the
//! web's persisted store, the list outlives a restart.

use serde_json::{Value, json};

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum View {
    Files(String),
    Diff,
    Browser(String),
    PullRequests,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Closed {
    pub thread: String,
    pub view: View,
}

const CAPACITY: usize = 20;
const FILE: &str = "closed-views.json";

#[derive(Default)]
pub struct Store {
    entries: Vec<Closed>,
    loaded: bool,
}

fn encode(entries: &[Closed]) -> Value {
    let rows: Vec<Value> = entries
        .iter()
        .map(|c| match &c.view {
            View::Files(path) => json!({"thread": c.thread, "kind": "files", "path": path}),
            View::Diff => json!({"thread": c.thread, "kind": "diff"}),
            View::PullRequests => json!({"thread": c.thread, "kind": "pull-requests"}),
            View::Browser(url) => json!({"thread": c.thread, "kind": "browser", "url": url}),
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
                "files" => View::Files(row["path"].as_str()?.to_owned()),
                "diff" => View::Diff,
                "pull-requests" => View::PullRequests,
                "browser" => View::Browser(row["url"].as_str()?.to_owned()),
                _ => return None,
            };
            Some(Closed { thread, view })
        })
        .take(CAPACITY)
        .collect()
}

impl Store {
    fn path() -> Option<std::path::PathBuf> {
        (!cfg!(test)).then(|| crate::settings::client::sibling_path(FILE))
    }

    fn load(&mut self) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        if let Some(saved) = Self::path().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
            self.entries = decode(&saved);
        }
    }

    fn save(&self) {
        let Some(path) = Self::path() else { return };
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        let written = serde_json::to_vec(&encode(&self.entries)).ok().is_some_and(|bytes| {
            path.parent().is_none_or(|p| std::fs::create_dir_all(p).is_ok()) && std::fs::write(&temporary, bytes).is_ok() && std::fs::rename(&temporary, &path).is_ok()
        });
        if !written {
            eprintln!("panel: saving closed views failed");
        }
    }

    /// A view that reopens to the same target replaces its older entry.
    pub fn record(&mut self, closed: Closed) {
        if closed.thread.is_empty() {
            return;
        }
        self.load();
        self.entries.retain(|c| *c != closed);
        self.entries.insert(0, closed);
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
        let entries = vec![closed("a", View::Files("src/lib.rs".into())), closed("b", View::Diff), closed("c", View::Browser("http://localhost:5173/".into())), closed("d", View::PullRequests)];
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
}
