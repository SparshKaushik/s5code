//! Recently closed panel views for `view.reopenClosed` (the web's `closedViewStore` and
//! `ReopenClosedViewShortcut`). Terminals are never recorded; newest first, at most 20.

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum View {
    Files(String),
    Diff,
    Browser(String),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Closed {
    pub thread: String,
    pub view: View,
}

const CAPACITY: usize = 20;

#[derive(Default)]
pub struct Store(Vec<Closed>);

impl Store {
    /// A view that reopens to the same target replaces its older entry.
    pub fn record(&mut self, closed: Closed) {
        if closed.thread.is_empty() {
            return;
        }
        self.0.retain(|c| *c != closed);
        self.0.insert(0, closed);
        self.0.truncate(CAPACITY);
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `planNextReopen`: drops entries whose thread is gone or that are open already, and
    /// takes the newest that is left.
    pub fn next(&mut self, exists: impl Fn(&str) -> bool, open: impl Fn(&Closed) -> bool) -> Option<Closed> {
        while !self.0.is_empty() {
            let c = self.0.remove(0);
            if exists(&c.thread) && !open(&c) {
                return Some(c);
            }
        }
        None
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
        assert_eq!(store.0.len(), 20);
        assert_eq!(store.0[0].view, View::Files("24".into()));
    }
}
