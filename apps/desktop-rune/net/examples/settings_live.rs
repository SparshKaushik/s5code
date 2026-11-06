//! Reversible settings RPC checks against the existing VM server (no agent turns).
use s5_net::Net;
use serde_json::{Value, json};
use std::{
    thread,
    time::{Duration, Instant},
};
fn wait(net: &mut Net, mut ready: impl FnMut(&mut Net) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        net.drain();
        if ready(net) {
            return;
        }
        assert!(Instant::now() < deadline, "RPC timed out");
        thread::park_timeout(Duration::from_millis(50));
    }
}
fn call(net: &mut Net, method: &str, payload: Value) -> Value {
    let id = net.request(method, payload);
    let mut reply = None;
    wait(net, |n| {
        reply = n.take_reply(id);
        reply.is_some()
    });
    let result = reply.unwrap().unwrap_or_else(|e| panic!("{method}: {e}"));
    println!("{method}: success");
    result
}
fn main() {
    let base = std::env::var("S5_BASE").unwrap_or("http://127.0.0.1:13773".into());
    let token =
        std::fs::read_to_string(format!("{}/s5token", std::env::var("HOME").unwrap())).unwrap();
    let ui = thread::current();
    let mut net = Net::connect(&base, token.trim(), move || ui.unpark());
    wait(&mut net, |n| n.state().config.is_some());
    let original =
        net.state().config.as_ref().unwrap()["settings"]["enableProviderUpdateChecks"].clone();
    let next = !original.as_bool().unwrap();
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"enableProviderUpdateChecks":next}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["enableProviderUpdateChecks"] == next
    });
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"enableProviderUpdateChecks":original}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["enableProviderUpdateChecks"] == original
    });
    println!("settings subscription observed write and restoration");
    call(
        &mut net,
        "server.refreshProviders",
        json!({"instanceId":"devReplay","fresh":true}),
    );
    let original_provider =
        net.state().config.as_ref().unwrap()["settings"]["providerInstances"]["devReplay"].clone();
    let mut provider = original_provider.clone();
    provider["displayName"] = json!("Replay settings verification");
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"providerInstances":{"devReplay":provider}}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["providerInstances"]["devReplay"]["displayName"]
            == "Replay settings verification"
    });
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"providerInstances":{"devReplay":original_provider}}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["providerInstances"]["devReplay"]
            == original_provider
    });
    println!("provider instance configuration written and restored");
    let project = net.state().projects[0]["id"].as_str().unwrap().to_owned();
    let original_override =
        net.state().config.as_ref().unwrap()["settings"]["projectSettingsOverrides"][&project]
            .clone();
    let mut entry = if original_override.is_object() {
        original_override.clone()
    } else {
        json!({})
    };
    entry["enableAgentBrowserAccess"] = json!(false);
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"projectSettingsOverrides":{&project:entry}}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["projectSettingsOverrides"][&project]["enableAgentBrowserAccess"]
            == false
    });
    call(
        &mut net,
        "server.updateSettings",
        json!({"patch":{"projectSettingsOverrides":{&project:original_override}}}),
    );
    wait(&mut net, |n| {
        n.state().config.as_ref().unwrap()["settings"]["projectSettingsOverrides"][&project]
            == original_override
    });
    println!("project override written and reset to inheritance");
    for method in ["scheduledTasks.subscribe", "subscribeAuthAccess"] {
        net.watch_resource(method, json!({}));
        wait(&mut net, |n| n.resource(method, json!({})).is_some());
        assert!(
            net.resource(method, json!({}))
                .unwrap()
                .get("error")
                .is_none(),
            "subscription {method} failed"
        );
        println!("{method}: snapshot received");
        net.unwatch_resource(method, json!({}));
    }
    let id = net.http_request("GET", "/api/auth/session", json!({}));
    let mut session = None;
    wait(&mut net, |n| {
        session = n.take_reply(id);
        session.is_some()
    });
    assert_eq!(session.unwrap().unwrap()["authenticated"], true);
    println!("authenticated HTTP session read succeeded");
    let key = "ctrl+alt+shift+f12";
    let command = "sidebar.toggle";
    let target = json!({"key":key,"command":command,"when":"isDesktop && !terminalFocus"});
    let added = call(&mut net, "server.upsertKeybinding", target.clone());
    assert!(added["keybindings"].to_string().contains("f12"));
    call(&mut net, "server.removeKeybinding", target);
    println!("keybinding upsert read back; temporary binding removed; no agents started");
}
