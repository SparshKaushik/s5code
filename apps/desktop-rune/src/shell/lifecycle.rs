//! The standing `subscribeServerLifecycle` subscription (client-runtime `legacyThreadMigration`):
//! the primary server's stays open, every open one's events wait here for their reader (the
//! server update), and a running legacy thread migration shows web's loading toast
//! (`LegacyThreadMigrationToast`).

use std::collections::HashMap;

use serde_json::{Value, json};

use crate::shell::toast::{self as stack, Kind};
use crate::{App, data, env};

pub const METHOD: &str = "subscribeServerLifecycle";
/// Events kept per environment for a reader that hasn't asked yet; the server replays the latest
/// of each type on subscribe, so a handful is plenty.
const KEEP: usize = 16;

#[derive(Default)]
pub struct Ui {
    events: HashMap<String, Vec<Value>>,
    /// The primary server's migration: its thread count while running.
    migrating: Option<u64>,
    toast: Option<u64>,
}

/// Takes the events environment `key` sent since its reader last asked.
pub fn take(ui: &mut Ui, key: &str) -> Vec<Value> {
    ui.events.remove(key).unwrap_or_default()
}

pub fn poll(s: &mut App) -> bool {
    let keys = env::keys(s);
    s.ui.shell.lifecycle.events.retain(|k, _| keys.contains(k));
    let mut migration = None;
    for key in keys {
        let Some(net) = env::net_of(s, &key) else { continue };
        let primary = key == env::LOCAL;
        if primary && !net.watching(METHOD, json!({})) {
            net.watch_resource(METHOD, json!({}));
        }
        let fresh = net.take_resource_events(METHOD, json!({}));
        if fresh.is_empty() {
            continue;
        }
        if primary && let Some(event) = fresh.iter().rev().find(|e| data::s(e, "type") == "legacyThreadMigration") {
            migration = Some(event["payload"].clone());
        }
        let kept = s.ui.shell.lifecycle.events.entry(key).or_default();
        kept.extend(fresh);
        let over = kept.len().saturating_sub(KEEP);
        kept.drain(..over);
    }
    let Some(payload) = migration else { return false };
    let running = (data::s(&payload, "status") == "running").then(|| payload["totalThreadCount"].as_u64().unwrap_or(0));
    if running == s.ui.shell.lifecycle.migrating {
        return false;
    }
    s.ui.shell.lifecycle.migrating = running;
    show(s, running);
    true
}

/// `startLegacyThreadMigrationToast`: one loading toast while the migration runs.
fn show(s: &mut App, running: Option<u64>) {
    match (running, s.ui.shell.lifecycle.toast) {
        (Some(count), None) => {
            let description = format!(
                "Migrating {} {} from the previous version. You can keep working while this finishes.",
                grouped(count),
                if count == 1 { "thread" } else { "threads" }
            );
            let toast = stack::Toast::new("Restoring your threads…", Some(&description), Kind::Loading);
            s.ui.shell.lifecycle.toast = Some(stack::add(s, toast, None));
        }
        (None, Some(id)) => {
            stack::close(s, id);
            s.ui.shell.lifecycle.toast = None;
        }
        _ => {}
    }
}

/// `Number.prototype.toLocaleString` in en-US: thousands grouped by commas.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn scene(s: &mut App) {
    s.ui.shell.lifecycle.migrating = Some(1284);
    show(s, Some(1284));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Source;
    use crate::shell::palette::{toast_description, toast_title};

    fn send(s: &mut App, event: Value) {
        let net = s.src.net().expect("live");
        net.put_resource_item(METHOD, json!({}), event);
    }

    #[test]
    fn a_running_migration_toasts_until_it_completes_and_events_wait_for_their_reader() {
        let mut s = App::new();
        s.src = Source::Live(s5_net::Net::in_memory("http://local.test"));
        poll(&mut s);
        assert!(s.src.net().expect("live").watching(METHOD, json!({})), "FAILED: the primary server's lifecycle stays subscribed");

        send(&mut s, json!({"type": "welcome", "payload": {}}));
        send(&mut s, json!({"type": "legacyThreadMigration", "payload": {"status": "running", "totalThreadCount": 1284}}));
        assert!(poll(&mut s));
        assert_eq!(toast_title(&s), Some("Restoring your threads…"));
        assert_eq!(toast_description(&s), Some("Migrating 1,284 threads from the previous version. You can keep working while this finishes."));
        let events = take(&mut s.ui.shell.lifecycle, env::LOCAL);
        assert_eq!(events.len(), 2, "FAILED: the update flow still reads every event");
        assert!(take(&mut s.ui.shell.lifecycle, env::LOCAL).is_empty());

        send(&mut s, json!({"type": "legacyThreadMigration", "payload": {"status": "running", "totalThreadCount": 1284}}));
        assert!(!poll(&mut s), "FAILED: a replayed running event keeps the one toast");
        send(&mut s, json!({"type": "legacyThreadMigration", "payload": {"status": "complete", "totalThreadCount": 1284}}));
        assert!(poll(&mut s));
        assert_eq!(toast_title(&s), None, "FAILED: completion closes the toast");
        assert_eq!(grouped(1), "1");
        assert_eq!(grouped(1_234_567), "1,234,567");
    }
}
