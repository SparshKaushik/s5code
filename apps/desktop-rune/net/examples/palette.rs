//! Read-only live verification: S5_BASE and S5_TOKEN, or the VM's ~/s5token.
use s5_net::Net;
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};
fn await_result<T>(net: &mut Net, mut ready: impl FnMut(&mut Net) -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        net.drain();
        if let Some(value) = ready(net) {
            return value;
        }
        assert!(Instant::now() < deadline, "server did not answer");
        thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
    }
}
fn main() {
    let base = std::env::var("S5_BASE").unwrap_or("http://127.0.0.1:13773".into());
    let token = std::env::var("S5_TOKEN").unwrap_or_else(|_| {
        std::fs::read_to_string(format!("{}/s5token", std::env::var("HOME").unwrap())).unwrap()
    });
    let current = thread::current();
    let mut net = Net::connect(&base, token.trim(), move || current.unpark());
    await_result(&mut net, |n| {
        (n.state().shell_ready && n.state().config.is_some()).then_some(())
    });
    println!(
        "shell/config ready: {} projects, {} threads",
        net.state().projects.len(),
        net.state().threads.len()
    );
    let id = net.state().threads.first().unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    net.watch_thread(&id);
    await_result(&mut net, |n| {
        n.state()
            .thread_views
            .get(&id)
            .filter(|v| v.synchronized)
            .map(|_| ())
    });
    println!("thread navigation: projection synchronized");
    for query in ["showcase", "replay", "settings"] {
        let id = net.request(
            "orchestration.searchThreads",
            json!({"query":query,"limit":10}),
        );
        let response: Value =
            await_result(&mut net, |n| n.take_reply(id)).expect("search RPC failed");
        let matches = response["matches"].as_array().expect("typed matches");
        println!("message search {query:?}: {} matches", matches.len());
    }
    if std::env::args().any(|arg| arg == "--restart-smoke") {
        let provider = net.state().config.as_ref().unwrap()["providers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| {
                p["models"]
                    .as_array()
                    .is_some_and(|models| models.iter().any(|m| m["slug"] == "showcase"))
            })
            .expect("Replay dev provider is required")["instanceId"]
            .clone();
        let project = net.state().projects[0].clone();
        let launched = net.launch_thread(
            json!({"projectId":project["id"],"title":"Kit palette restart smoke",
            "modelSelection":{"instanceId":provider,"model":"showcase"},"runtimeMode":"full-access",
            "interactionMode":"default","workspaceStrategy":{"type":"root"},
            "initialMessage":{"text":"Replay only: verify palette restart","attachments":[]}}),
        );
        let result =
            await_result(&mut net, |n| n.take_reply(launched)).expect("launch Replay thread");
        let test_id = result["threadId"].as_str().unwrap().to_owned();
        net.watch_thread(&test_id);
        await_result(&mut net, |n| {
            n.state()
                .thread_views
                .get(&test_id)
                .filter(|view| {
                    let runs = view.projection["runs"].as_array();
                    view.synchronized
                        && runs.is_some_and(|runs| {
                            !runs.is_empty() && runs.iter().all(|r| r["status"] == "completed")
                        })
                })
                .map(|_| ())
        });
        let sessions = net.state().thread_views[&test_id].projection["providerSessions"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        assert!(!sessions.is_empty(), "Replay should attach a session");
        for session in &sessions {
            let command=net.dispatch(json!({"type":"provider-session.detach","threadId":test_id,"providerSessionId":session["id"],"reason":"client-requested"}));
            await_result(&mut net, |n| n.take_reply(command)).expect("detach Replay session");
        }
        let refresh = net.request(
            "server.refreshProviders",
            json!({"instanceId":provider,"cwd":project["workspaceRoot"],"fresh":true}),
        );
        await_result(&mut net, |n| n.take_reply(refresh)).expect("refresh Replay metadata");
        println!(
            "restart: {} Replay sessions detached; provider metadata refreshed",
            sessions.len()
        );
        let cleanup = net.dispatch(json!({"type":"thread.delete","threadId":test_id}));
        await_result(&mut net, |n| n.take_reply(cleanup)).expect("delete owned smoke thread");
        net.unwatch_thread(&test_id);
        println!("owned Replay test thread deleted");
    }
    net.unwatch_thread(&id);
    println!("palette RPC verification passed");
}
