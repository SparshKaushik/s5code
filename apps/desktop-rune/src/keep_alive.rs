//! RunningThreadKeepAlive (Electron's `runningThreadKeepAliveAtom`): each environment follows
//! the details of its running threads, opened or not. A thread that stopped stays followed until
//! its own stream is synchronized and reports no running run, then is let go unless something
//! else watches it.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::{App, data, env};

#[derive(Default)]
pub struct Ui {
    /// Threads followed, by environment.
    kept: HashMap<String, HashSet<String>>,
}

/// `isRunning`.
fn running(status: &str) -> bool {
    matches!(status, "preparing" | "queued" | "starting" | "running")
}

/// `isDetailDone`: no stream, a thread gone from the shell (web's `deleted`), or a synchronized
/// stream with no run in progress.
fn detail_done(view: Option<&s5_net::ThreadView>, listed: bool) -> bool {
    let Some(view) = view.filter(|_| listed) else { return true };
    view.synchronized && !view.projection["runs"].as_array().into_iter().flatten().any(|r| running(data::s(r, "status")))
}

/// The threads to follow: the running ones, and those that stopped whose stream hasn't caught up.
fn next_kept(threads: &[Value], previous: &HashSet<String>, done: impl Fn(&str) -> bool) -> HashSet<String> {
    let mut kept: HashSet<String> = threads.iter().filter(|t| running(data::s(t, "status"))).map(|t| data::s(t, "id").to_owned()).collect();
    let stopped: Vec<String> = previous.iter().filter(|id| !kept.contains(*id) && !done(id)).cloned().collect();
    kept.extend(stopped);
    kept
}

pub fn poll(s: &mut App) {
    let keys = env::keys(s);
    s.ui.keep_alive.kept.retain(|key, _| keys.contains(key));
    for key in keys {
        let previous = s.ui.keep_alive.kept.remove(&key).unwrap_or_default();
        let Some(net) = env::net_of(s, &key).filter(|n| n.state().shell_ready) else {
            s.ui.keep_alive.kept.insert(key, previous);
            continue;
        };
        let state = net.state();
        let listed = |id: &str| state.threads.iter().chain(&state.archived).any(|t| t["id"] == id);
        let next = next_kept(&state.threads, &previous, |id| detail_done(state.thread_views.get(id), listed(id)));
        for id in previous.difference(&next) {
            net.release_thread(id);
        }
        for id in &next {
            net.keep_thread(id);
        }
        s.ui.keep_alive.kept.insert(key, next);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::data::Source;

    fn row(id: &str, status: &str) -> Value {
        json!({"id": id, "status": status})
    }

    fn state(s: &mut App) -> &mut s5_net::State {
        s.src.net().expect("live").state_mut()
    }

    fn views(s: &mut App) -> Vec<String> {
        let mut ids: Vec<String> = state(s).thread_views.keys().cloned().collect();
        ids.sort();
        ids
    }

    #[test]
    fn running_threads_are_followed_until_their_own_stream_settles() {
        let mut s = App::new();
        s.src = Source::Live(s5_net::Net::in_memory("http://local.test"));
        state(&mut s).shell_ready = true;
        state(&mut s).threads = vec![row("run", "running"), row("idle", "idle")];
        poll(&mut s);
        assert_eq!(views(&mut s), ["run"], "FAILED: a running thread never opened is followed; an idle one is not");

        state(&mut s).threads = vec![row("run", "idle"), row("idle", "idle")];
        let view = state(&mut s).thread_views.get_mut("run").expect("followed");
        view.projection = json!({"runs": [{"status": "running"}]});
        view.synchronized = true;
        poll(&mut s);
        assert_eq!(views(&mut s), ["run"], "FAILED: kept while its own stream still shows a run");

        state(&mut s).thread_views.get_mut("run").expect("followed").projection = json!({"runs": [{"status": "completed"}]});
        poll(&mut s);
        assert!(views(&mut s).is_empty(), "FAILED: released once its stream settles");

        // A thread the user opened while it ran stays followed after it settles.
        state(&mut s).threads = vec![row("open", "running")];
        poll(&mut s);
        s.src.net().expect("live").watch_thread("open");
        state(&mut s).threads = vec![row("open", "idle")];
        let view = state(&mut s).thread_views.get_mut("open").expect("followed");
        view.synchronized = true;
        poll(&mut s);
        assert_eq!(views(&mut s), ["open"], "FAILED: an opened thread is not released by the keep-alive");
    }

    #[test]
    fn a_stopped_thread_waits_for_its_stream() {
        let previous: HashSet<String> = ["a", "b", "c"].map(String::from).into();
        let threads = [row("a", "starting"), row("b", "idle")];
        let kept = next_kept(&threads, &previous, |id| id == "c");
        assert_eq!(kept, ["a", "b"].map(String::from).into(), "FAILED: running kept, b's stream not done, c released");
        let synced = s5_net::ThreadView { synchronized: true, ..Default::default() };
        assert!(detail_done(None, true), "FAILED: no stream counts as done");
        assert!(detail_done(Some(&synced), false), "FAILED: a thread gone from the shell is done");
        assert!(!detail_done(Some(&s5_net::ThreadView::default()), true), "FAILED: an unsynchronized stream is not done yet");
        assert!(detail_done(Some(&synced), true));
    }
}
