//! SlowRpcRequestToastCoordinator over web's `requestLatencyState`: one warning toast lists the
//! requests any environment has left unanswered past their threshold, until each is answered.

use rune::Tx;

use crate::shell::toast::{self as stack, Kind};
use crate::{App, env};

/// `SLOW_RPC_ACK_THRESHOLD_MS`.
const SLOW_MS: i64 = 15_000;
/// `LONG_RUNNING_RPC_ACK_THRESHOLD_MS`: package installs and background VCS work.
const LONG_RUNNING_MS: i64 = 120_000;
/// Background reports nobody waits on, and a sign-in that lasts as long as the user takes.
const UNTRACKED: [&str; 3] = ["server.getUsageSummary", "server.reportClientActivity", "sourceControl.signInWait"];
const LONG_RUNNING: [&str; 4] = ["server.updateProvider", "server.refreshProviders", "server.updateServer", "vcs.refreshStatus"];

#[derive(Default)]
pub struct Ui {
    /// The toast and the requests it lists, by environment and id.
    toast: Option<(u64, Vec<(String, u64)>)>,
}

/// `rpcAckThresholdMs`, or `None` for a request web never warns about (`shouldTrackRpcAck`).
fn threshold(method: &str) -> Option<i64> {
    if method.contains("subscribe") || method.starts_with("pullRequests.") || UNTRACKED.contains(&method) {
        return None;
    }
    Some(if LONG_RUNNING.contains(&method) { LONG_RUNNING_MS } else { SLOW_MS })
}

struct Slow {
    key: String,
    id: u64,
    tag: String,
    sent: i64,
    threshold: i64,
}

/// The requests past their threshold, oldest first, and when the next one will be.
fn slow(s: &mut App, now: i64) -> (Vec<Slow>, Option<i64>) {
    let (mut out, mut next) = (Vec::new(), None::<i64>);
    for key in env::keys(s) {
        let label = env::label(s, &key);
        let Some(net) = env::net_of(s, &key) else { continue };
        for (id, method, sent) in net.awaiting() {
            let Some(threshold) = threshold(method) else { continue };
            if now - sent >= threshold {
                out.push(Slow { key: key.clone(), id, tag: format!("{method} · {label}"), sent, threshold });
            } else {
                next = Some(next.map_or(sent + threshold, |n| n.min(sent + threshold)));
            }
        }
    }
    out.sort_by_key(|r| (r.sent, r.id));
    (out, next)
}

/// `describeSlowRequests`.
fn describe(requests: &[Slow]) -> String {
    let count = requests.len();
    let seconds = requests.iter().map(|r| r.threshold).min().unwrap_or(SLOW_MS) as f64 / 1000.0;
    format!("{count} request{} waiting longer than {}s.", if count == 1 { "" } else { "s" }, seconds.round() as i64)
}

/// Web's `toLocaleTimeString()` in en-US: "3:04:05 PM".
fn clock(at: i64) -> String {
    let seconds = crate::data::Zone::Local.wall(at).div_euclid(1000).rem_euclid(24 * 3600);
    let (hour, minute, second) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    format!("{}:{minute:02}:{second:02} {}", (hour + 11) % 12 + 1, if hour < 12 { "AM" } else { "PM" })
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    // A fixture has no connection to wait on; its scene stages the toast itself.
    if matches!(s.src, crate::data::Source::Fixture(_)) {
        return false;
    }
    let now = crate::data::now_ms();
    let (requests, next) = slow(s, now);
    if let Some(at) = next {
        tx.wake_after(std::time::Duration::from_millis((at - now).max(1) as u64));
    }
    show(s, &requests)
}

fn show(s: &mut App, requests: &[Slow]) -> bool {
    let ids: Vec<(String, u64)> = requests.iter().map(|r| (r.key.clone(), r.id)).collect();
    let current = s.ui.shell.slow_requests.toast.take();
    if requests.is_empty() {
        if let Some((id, _)) = current {
            stack::close(s, id);
            return true;
        }
        return false;
    }
    if current.as_ref().is_some_and(|(_, shown)| *shown == ids) {
        s.ui.shell.slow_requests.toast = current;
        return false;
    }
    let mut toast = stack::Toast::new("Some requests are slow", Some(&describe(requests)), Kind::Warning);
    toast.details = Some(requests.iter().map(|r| format!("{}\nStarted {}", r.tag, clock(r.sent))).collect::<Vec<_>>().join("\n\n"));
    toast.details_labels = Some(("Show requests", "Hide requests"));
    let id = match current {
        // `toastManager.update` leaves a toast the user closed closed.
        Some((id, _)) => {
            stack::update(s, id, toast, None);
            id
        }
        None => stack::add(s, toast, None),
    };
    s.ui.shell.slow_requests.toast = Some((id, ids));
    true
}

/// Two requests left waiting, the list open.
pub fn scene(s: &mut App) {
    let now = crate::data::now_ms();
    let slow = |id, tag: &str, waited| Slow { key: env::LOCAL.to_owned(), id, tag: tag.to_owned(), sent: now - waited, threshold: SLOW_MS };
    show(s, &[slow(1, "git.pull · This machine", 48_000), slow(2, "projects.searchEntries · This machine", 21_000)]);
    if let Some(toast) = stack::newest_mut(s) {
        toast.details_open = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::palette::{toast_description, toast_title};

    #[test]
    fn thresholds_follow_web_s_tracked_methods() {
        assert_eq!(threshold("git.pull"), Some(SLOW_MS));
        assert_eq!(threshold("server.updateProvider"), Some(LONG_RUNNING_MS));
        assert_eq!(threshold("vcs.refreshStatus"), Some(LONG_RUNNING_MS));
        assert_eq!(threshold("subscribeServerLifecycle"), None);
        assert_eq!(threshold("pullRequests.list"), None);
        assert_eq!(threshold("server.getUsageSummary"), None);
    }

    #[test]
    fn one_toast_lists_the_slow_requests_until_they_are_answered() {
        let mut s = App::new();
        s.src = crate::data::Source::Live(s5_net::Net::in_memory("http://local.test"));
        let net = s.src.net().expect("live");
        net.request("git.pull", serde_json::json!({}));
        let _update = net.request("server.updateProvider", serde_json::json!({}));
        let pull_sent = net.awaiting().find(|r| r.1 == "git.pull").map(|r| r.2).expect("sent");
        let update_sent = net.awaiting().find(|r| r.1 == "server.updateProvider").map(|r| r.2).expect("sent");
        let sent = pull_sent;

        let (requests, next) = slow(&mut s, sent + SLOW_MS - 1);
        assert!(requests.is_empty());
        assert_eq!(next, Some(sent + SLOW_MS), "FAILED: wakes when the first one turns slow");

        let (requests, next) = slow(&mut s, sent + SLOW_MS + 1);
        let tag = format!("git.pull · {}", env::label(&s, env::LOCAL));
        assert_eq!(requests.iter().map(|r| r.tag.as_str()).collect::<Vec<_>>(), [tag.as_str()]);
        assert_eq!(next, Some(update_sent + LONG_RUNNING_MS), "FAILED: a provider update waits two minutes");
        assert!(show(&mut s, &requests));
        assert_eq!(toast_title(&s), Some("Some requests are slow"));
        assert_eq!(toast_description(&s), Some("1 request waiting longer than 15s."));
        assert!(!show(&mut s, &requests), "FAILED: the same set leaves the toast alone");

        let (requests, _) = slow(&mut s, update_sent + LONG_RUNNING_MS);
        assert!(show(&mut s, &requests));
        assert_eq!(toast_description(&s), Some("2 requests waiting longer than 15s."));

        assert!(show(&mut s, &[]));
        assert_eq!(toast_title(&s), None, "FAILED: no slow request, no toast");
    }
}
