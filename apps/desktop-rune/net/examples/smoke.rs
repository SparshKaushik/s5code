//! Exercises real subscriptions and commands against a disposable dev server.
use s5_net::{Conn, Net, uuid};
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};
fn pump(net: &mut Net, label: &str, mut done: impl FnMut(&mut Net) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        net.drain();
        if done(net) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timeout: {label}; connection {:?}",
            net.state().conn
        );
        thread::park_timeout(Duration::from_millis(50));
    }
}
fn reply(net: &mut Net, id: u64) -> Value {
    let mut result = None;
    pump(net, "RPC reply", |n| {
        result = n.take_reply(id);
        result.is_some()
    });
    result
        .unwrap()
        .unwrap_or_else(|e| panic!("RPC failure: {e}"))
}
fn turn(net: &mut Net, id: &str, expected_runs: usize) {
    let mut last = String::new();
    pump(net, "turn completion", |n| {
        let Some(view) = n.state().thread_views.get(id) else {
            return false;
        };
        let p = &view.projection;
        let runs = p["runs"].as_array().cloned().unwrap_or_default();
        let statuses: Vec<_> = runs
            .iter()
            .map(|r| r["status"].as_str().unwrap_or("?"))
            .collect();
        let summary = format!(
            "runs={statuses:?} messages={} items={}",
            p["messages"].as_array().map_or(0, Vec::len),
            p["visibleTurnItems"].as_array().map_or(0, Vec::len)
        );
        if summary != last {
            println!("  {summary} seq={}", view.sequence);
            last = summary;
        }
        view.synchronized
            && runs.len() >= expected_runs
            && statuses
                .iter()
                .all(|s| matches!(*s, "completed" | "failed" | "cancelled" | "interrupted"))
    });
    let p = &net.state().thread_views[id].projection;
    assert!(
        p["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "completed"),
        "turn failed: {}",
        p["runs"]
    );
}
fn main() {
    let base = std::env::var("S5_BASE").unwrap_or("http://127.0.0.1:13773".into());
    let token = std::env::var("S5_TOKEN").unwrap_or_else(|_| {
        std::fs::read_to_string(format!("{}/s5token", std::env::var("HOME").unwrap())).unwrap()
    });
    let ui = thread::current();
    let mut net = Net::connect(&base, token.trim(), move || ui.unpark());
    pump(&mut net, "shell/config", |n| {
        n.state().shell_ready && n.state().config.is_some()
    });
    println!(
        "ready: {} projects, {} active threads, {} archived",
        net.state().projects.len(),
        net.state().threads.len(),
        net.state().archived.len()
    );
    let existing = net
        .state()
        .threads
        .iter()
        .chain(&net.state().archived)
        .max_by_key(|t| t["itemCount"].as_u64().unwrap_or(0))
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    net.watch_thread(&existing);
    pump(&mut net, "existing projection", |n| {
        n.state().thread_views[&existing].synchronized
    });
    println!(
        "existing thread: {} messages",
        net.state().thread_views[&existing].projection["messages"]
            .as_array()
            .unwrap()
            .len()
    );
    net.unwatch_thread(&existing);
    let project = net.state().projects[0]["id"].clone();
    let config = net.state().config.as_ref().unwrap();
    let provider = config["providers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p["models"]
                .as_array()
                .is_some_and(|models| models.iter().any(|m| m["slug"] == "showcase"))
        })
        .expect("devReplay provider")["instanceId"]
        .clone();
    let launch = net.launch_thread(json!({"projectId":project,"title":"Rune net smoke","modelSelection":{"instanceId":provider,"model":"showcase"},"runtimeMode":"full-access","interactionMode":"default","workspaceStrategy":{"type":"root"},"initialMessage":{"text":"Showcase the Rune client","attachments":[]}}));
    let result = reply(&mut net, launch);
    let id = result["threadId"].as_str().unwrap().to_owned();
    println!("launched {id}");
    net.watch_thread(&id);
    turn(&mut net, &id, 1);
    let follow = net.dispatch(json!({"type":"message.dispatch","threadId":id,"messageId":uuid(),"text":"One more showcase","attachments":[],"createdBy":"user","creationSource":"web","dispatchMode":{"type":"start_immediately"}}));
    reply(&mut net, follow);
    println!("follow-up:");
    turn(&mut net, &id, 2);
    for (ty, pinned) in [("thread.pin", true), ("thread.unpin", false)] {
        let request = net.dispatch(json!({"type":ty,"threadId":id}));
        reply(&mut net, request);
        pump(&mut net, ty, |n| {
            n.state()
                .threads
                .iter()
                .any(|t| t["id"] == id && !t["pinnedAt"].is_null() == pinned)
        });
        println!(
            "{ty}: pinnedAt={}",
            net.state().threads.iter().find(|t| t["id"] == id).unwrap()["pinnedAt"]
        );
    }
    let ui = thread::current();
    let mut peer = Net::connect(&base, token.trim(), move || ui.unpark());
    pump(&mut peer, "peer connection", |n| n.state().shell_ready);
    let sequence = net.state().thread_views[&id].sequence;
    net.reconnect();
    pump(&mut net, "socket disconnected", |n| {
        n.state().conn != Conn::Open
    });
    let change = peer.dispatch(json!({"type":"thread.pin","threadId":id}));
    let changed_sequence = reply(&mut peer, change)["sequence"].as_u64().unwrap();
    pump(&mut net, "resubscriptions", |n| {
        n.state().conn == Conn::Open
            && n.state().shell_ready
            && n.state().thread_views[&id].synchronized
            && n.state().thread_views[&id].sequence >= changed_sequence
            && n.state()
                .threads
                .iter()
                .any(|t| t["id"] == id && !t["pinnedAt"].is_null())
    });
    assert!(net.state().thread_views[&id].sequence >= sequence);
    println!(
        "reconnected: shell synchronized, missed pin delivered, thread resumed at {} (was {sequence})",
        net.state().thread_views[&id].sequence
    );
    let cleanup = net.dispatch(json!({"type":"thread.unpin","threadId":id}));
    reply(&mut net, cleanup);
}
