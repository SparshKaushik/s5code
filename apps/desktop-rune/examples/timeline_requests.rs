//! Exercises real subscriptions and commands against a disposable dev server.
use s5_net::Net;
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
#[path = "../src/chat/timeline/requests.rs"]
mod requests;
#[path = "../src/chat/timeline/rows.rs"]
mod rows;
mod data {
    pub fn s<'a>(v: &'a serde_json::Value, k: &str) -> &'a str {
        v[k].as_str().unwrap_or("")
    }
    pub fn ts(_: &serde_json::Value, _: &str) -> Option<i64> {
        None
    }
}
fn main() {
    let base = std::env::var("S5_SERVER_URL").unwrap_or("http://127.0.0.1:13773".into());
    let token = std::fs::read_to_string(
        std::env::var("S5_TOKEN_FILE")
            .unwrap_or_else(|_| format!("{}/s5token", std::env::var("HOME").unwrap())),
    )
    .unwrap();
    let ui = thread::current();
    let mut net = Net::connect(&base, token.trim(), move || ui.unpark());
    pump(&mut net, "shell", |n| {
        n.state().shell_ready && n.state().config.is_some()
    });
    let project = net.state().projects[0]["id"].clone();
    for (scenario, decision) in [
        ("approval", "accept"),
        ("approval", "decline"),
        ("question", ""),
    ] {
        let launch = net.launch_thread(json!({"projectId":project,"title":format!("Rune request verification {scenario} {decision}"),"modelSelection":{"instanceId":"devReplay","model":scenario},"runtimeMode":"approval-required","interactionMode":"default","workspaceStrategy":{"type":"root"},"initialMessage":{"text":"Exercise runtime request","attachments":[]}}));
        let launched = reply(&mut net, launch);
        let id = launched["threadId"].as_str().unwrap().to_owned();
        net.watch_thread(&id);
        let mut pending = Value::Null;
        pump(&mut net, "pending request", |n| {
            if let Some(view) = n.state().thread_views.get(&id) {
                if let Some(r) = view.projection["runtimeRequests"]
                    .as_array()
                    .and_then(|rs| rs.iter().find(|r| r["status"] == "pending"))
                {
                    pending = r.clone();
                    return true;
                }
            }
            false
        });
        let p = &net.state().thread_views[&id].projection;
        std::fs::write(
            format!("/tmp/rune-{scenario}-{decision}.json"),
            serde_json::to_string_pretty(p).unwrap(),
        )
        .unwrap();
        let item = p["visibleTurnItems"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| &v["item"])
            .find(|i| i["requestId"] == pending["id"])
            .unwrap();
        println!("{scenario}: kind={} item={}", pending["kind"], item["type"]);
        let fields = if scenario == "question" {
            let mut selected = std::collections::HashMap::new();
            for q in rows::array(item, "questions") {
                if let Some(o) = rows::array(q, "options").first() {
                    selected.insert(
                        data::s(q, "id").to_owned(),
                        vec![
                            o["value"]
                                .as_str()
                                .unwrap_or(data::s(o, "label"))
                                .to_owned(),
                        ],
                    );
                }
            }
            json!({"answers":requests::answers(item, &selected, |_| Some("Rune verified".into()))})
        } else {
            json!({"decision":decision})
        };
        let command = requests::command(&id, data::s(&pending, "id"), fields);
        println!(
            "dispatch {} decision={} answers={}",
            command["type"], command["decision"], command["answers"]
        );
        let rpc = net.dispatch(command);
        let receipt = reply(&mut net, rpc);
        println!("accepted: {receipt}");
        pump(&mut net, "resolved request", |n| {
            n.state().thread_views[&id].projection["runtimeRequests"]
                .as_array()
                .is_some_and(|rs| {
                    rs.iter()
                        .any(|r| r["id"] == pending["id"] && r["status"] != "pending")
                })
        });
        println!("request resolved");
        net.unwatch_thread(&id);
    }
}
