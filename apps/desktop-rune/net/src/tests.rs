use super::*;
fn projection() -> Value {
    json!({"thread":{"id":"t","title":"kept"},"updatedAt":"old","runs":[],"attempts":[],"turnItems":[],"visibleTurnItems":[]})
}
fn event(p: &mut Value, ty: &str, payload: Value) {
    projection::apply(
        p,
        &json!({"threadId":"t","type":ty,"payload":payload,"occurredAt":"now"}),
    );
}
fn item(id: &str, ordinal: i32) -> Value {
    json!({"id":id,"threadId":"t","ordinal":ordinal,"runId":"r","type":"command_execution","output":"done"})
}
#[test]
fn ordering_streaming_and_inheritance() {
    let mut p = projection();
    p["visibleTurnItems"] = json!([{"position":0,"visibility":"inherited","sourceItemId":"ancestor","item":item("ancestor",100)}]);
    event(&mut p, "turn-item.updated", item("b", 2));
    event(&mut p, "turn-item.updated", item("a", 1));
    assert_eq!(p["visibleTurnItems"][1]["item"]["id"], "a");
    let mut update = item("a", 1);
    update["output"] = json!("streamed");
    update["deferredWork"] = json!({"state":"pending"});
    event(&mut p, "turn-item.updated", update);
    event(&mut p, "turn-item.updated", item("a", 3));
    assert_eq!(p["visibleTurnItems"][2]["item"]["id"], "a");
    assert_eq!(p["visibleTurnItems"][2]["position"], 2);
    assert_eq!(p["turnItems"][1]["deferredWork"]["state"], "pending");
    event(
        &mut p,
        "run.updated",
        json!({"id":"r","status":"rolled_back"}),
    );
    assert_eq!(p["visibleTurnItems"].as_array().unwrap().len(), 1);
    assert_eq!(p["visibleTurnItems"][0]["sourceItemId"], "ancestor");
}
#[test]
fn visibility_changes_and_interrupt_pairing() {
    let mut p = projection();
    event(
        &mut p,
        "run.updated",
        json!({"id":"bad","status":"rolled_back"}),
    );
    event(&mut p, "turn-item.updated", item("a", 1));
    let mut hidden = item("a", 1);
    hidden["runId"] = json!("bad");
    event(&mut p, "turn-item.updated", hidden);
    assert!(p["visibleTurnItems"].as_array().unwrap().is_empty());
    event(&mut p, "turn-item.updated", item("a", 1));
    assert_eq!(p["visibleTurnItems"].as_array().unwrap().len(), 1);
    event(
        &mut p,
        "run-attempt.updated",
        json!({"id":"attempt","runId":"r","rootNodeId":"n","status":"superseded"}),
    );
    let mut result = item("interrupt", 2);
    result["type"] = json!("run_interrupt_result");
    result["nodeId"] = json!("n");
    event(&mut p, "turn-item.updated", result.clone());
    assert_eq!(p["visibleTurnItems"].as_array().unwrap().len(), 1);
    let mut request = item("request", 3);
    request["type"] = json!("run_interrupt_request");
    event(&mut p, "turn-item.updated", request);
    event(&mut p, "turn-item.updated", result);
    assert_eq!(p["visibleTurnItems"].as_array().unwrap().len(), 3);
    let mut queued = item("queued", 4);
    queued["type"] = json!("user_message");
    queued["inputIntent"] = json!("queued_turn");
    event(&mut p, "turn-item.updated", queued);
    event(
        &mut p,
        "run.updated",
        json!({"id":"r","status":"cancelled"}),
    );
    assert!(
        !projection::rows(&p, "visibleTurnItems")
            .iter()
            .any(|v| v["sourceItemId"] == "queued")
    );
}
#[test]
fn metadata_tokens_removals_and_deduped_bodies() {
    let mut p = projection();
    event(
        &mut p,
        "thread.visited",
        json!({"id":"t","lastVisitedAt":"now"}),
    );
    assert_eq!(p["updatedAt"], "old");
    event(
        &mut p,
        "provider-turn.updated",
        json!({"id":"pt","tokenUsage":{"total":42}}),
    );
    event(
        &mut p,
        "provider-turn.updated",
        json!({"id":"pt","status":"completed"}),
    );
    assert_eq!(p["providerTurns"][0]["tokenUsage"]["total"], 42);
    event(&mut p, "provider-session.attached", json!({"id":"ps"}));
    event(
        &mut p,
        "provider-session.detached",
        json!({"providerSessionId":"ps"}),
    );
    assert_eq!(p["providerSessions"], json!([]));
    event(&mut p, "message.updated", json!({"id":"m","text":"full"}));
    event(
        &mut p,
        "turn-item.updated",
        json!({"id":"reply","threadId":"t","type":"assistant_message","messageId":"m","text":"","textOmitted":true,"ordinal":1}),
    );
    assert_eq!(p["visibleTurnItems"][0]["item"]["text"], "full");
    assert!(p["turnItems"][0].get("textOmitted").is_none());
    event(
        &mut p,
        "subagent.updated",
        json!({"id":"s","prompt":"brief","result":null}),
    );
    event(
        &mut p,
        "subagent.updated",
        json!({"id":"s","prompt":"","promptOmitted":true,"result":null}),
    );
    assert_eq!(p["subagents"][0]["prompt"], "brief");
    let before = p.clone();
    projection::apply(
        &mut p,
        &json!({"threadId":"other","type":"message.updated","payload":{"id":"wrong"}}),
    );
    assert_eq!(p, before);
}
#[test]
fn shell_config_and_vcs() {
    let mut s = State::default();
    s.fold(Route::Shell,json!({"kind":"snapshot","snapshot":{"threads":[{"id":"t"}],"archivedThreads":[],"projects":[{"id":"p"}]}}));
    s.fold(Route::Shell, json!({"kind":"synchronized"}));
    assert!(s.shell_ready);
    s.fold(Route::Shell,json!({"kind":"thread.updated","location":"archive","thread":{"id":"t","title":"archived"}}));
    assert!(s.threads.is_empty());
    assert_eq!(s.archived.len(), 1);
    s.fold(
        Route::Shell,
        json!({"kind":"thread.removed","location":"archive","threadId":"t"}),
    );
    assert!(s.archived.is_empty());
    s.fold(
        Route::Shell,
        json!({"kind":"project.removed","projectId":"p"}),
    );
    assert!(s.projects.is_empty());
    s.fold(
        Route::Config,
        json!({"type":"snapshot","config":{"settings":{},"providers":[]}}),
    );
    for (ty, payload, key, expected) in [
        (
            "settingsUpdated",
            json!({"settings":{"a":true}}),
            "settings",
            json!({"a":true}),
        ),
        (
            "providerStatuses",
            json!({"providers":[1]}),
            "providers",
            json!([1]),
        ),
        (
            "keybindingsUpdated",
            json!({"keybindings":{},"issues":[]}),
            "issues",
            json!([]),
        ),
        (
            "environmentThemesUpdated",
            json!({"themes":[2]}),
            "environmentThemes",
            json!([2]),
        ),
        (
            "usageLimitSourcesUpdated",
            json!({"sources":[3]}),
            "usageLimitSources",
            json!([3]),
        ),
    ] {
        s.fold(Route::Config, json!({"type":ty,"payload":payload}));
        assert_eq!(s.config.as_ref().unwrap()[key], expected);
    }
    s.fold(
        Route::Vcs("/repo".into()),
        json!({"_tag":"snapshot","local":{"isRepo":true},"remote":{"aheadCount":2}}),
    );
    s.fold(
        Route::Vcs("/repo".into()),
        json!({"_tag":"localUpdated","local":{"isRepo":false}}),
    );
    assert_eq!(s.vcs["/repo"]["remote"]["aheadCount"], 2);
    s.fold(
        Route::Vcs("/repo".into()),
        json!({"_tag":"remoteUpdated","remote":null}),
    );
    assert!(s.vcs["/repo"]["remote"].is_null());
    s.fold(
        Route::Vcs("/repo".into()),
        json!({"error":{"_tag":"Failure","cause":{"message":"git failed"}}}),
    );
    assert_eq!(s.vcs["/repo"]["error"]["cause"]["message"], "git failed");
    s.fold(
        Route::Vcs("/repo".into()),
        json!({"_tag":"snapshot","local":{"isRepo":true},"remote":null}),
    );
    assert!(s.vcs["/repo"].get("error").is_none());
}
#[test]
fn pairing_and_uuid() {
    assert_eq!(
        parse_pairing_url("http://host:123/pair#token=a%2Bb").unwrap(),
        ("http://host:123".into(), "a+b".into())
    );
    assert!(parse_pairing_url("https://host/pair").is_err());
    let id = uuid();
    assert_eq!(id.len(), 36);
    assert_eq!(&id[14..15], "4");
}
#[test]
fn signed_upload_paths_join_the_server_origin() {
    assert_eq!(
        upload_url("http://host:3773", "/api/attachments/upload/abc.sig").unwrap(),
        "http://host:3773/api/attachments/upload/abc.sig"
    );
    assert_eq!(
        upload_url("https://relay.example/env/", "/api/attachments/upload/x").unwrap(),
        "https://relay.example/api/attachments/upload/x"
    );
    assert!(upload_url("http://host", "/api/auth/session").is_err());
    assert!(upload_url("http://host", "https://elsewhere/api/attachments/upload/x").is_err());
}
#[test]
fn enrichment_preserves_shell_rows_and_resume_sequence() {
    let mut s = State::default();
    s.fold(Route::Shell,json!({"kind":"snapshot","snapshot":{"snapshotSequence":10,"threads":[{"id":"t"}],"archivedThreads":[],"projects":[{"id":"p","workspaceRoot":"/p","title":"original","repositoryIdentity":null}]}}));
    s.fold(Route::Shell, json!({"kind":"synchronized"}));
    s.fold(Route::Shell,json!({"kind":"snapshot","resolvedRepositoryIdentityRoots":["/p"],"snapshot":{"snapshotSequence":20,"threads":[],"archivedThreads":[],"projects":[{"id":"p","workspaceRoot":"/p","title":"stale","repositoryIdentity":{"key":"repo"}}]}}));
    assert_eq!(s.threads.len(), 1);
    assert!(s.shell_ready);
    assert_eq!(s.shell_sequence, 10);
    assert_eq!(s.projects[0]["title"], "original");
    assert_eq!(s.projects[0]["repositoryIdentity"]["key"], "repo");
    s.fold(Route::Shell,json!({"kind":"project.updated","sequence":11,"project":{"id":"p","workspaceRoot":"/p","title":"new","repositoryIdentity":null}}));
    assert_eq!(s.projects[0]["repositoryIdentity"]["key"], "repo");
    s.fold(
        Route::Shell,
        json!({"kind":"thread.removed","sequence":9,"location":"active","threadId":"t"}),
    );
    assert_eq!(s.threads.len(), 1);
}
#[test]
fn compact_pull_request_update_preserves_other_links_and_metadata() {
    let mut p = projection();
    p["thread"]["pullRequests"] = json!([{"host":"github.com","repository":"Org/Repo","number":1},{"host":"github.com","repository":"org/repo","number":2}]);
    event(
        &mut p,
        "thread.pull-request-link-updated",
        json!({"host":"GitHub.com","repository":"org/repo","number":2,"source":"agent"}),
    );
    assert_eq!(p["thread"]["title"], "kept");
    assert_eq!(p["thread"]["pullRequests"][0]["number"], 1);
    assert_eq!(p["thread"]["pullRequests"][1]["source"], "agent");
}

#[test]
fn access_snapshot_keeps_other_clients_and_ignores_stale_events() {
    let mut state = json!({"type":"snapshot","revision":1,"payload":{"clientSessions":[{"sessionId":"a"},{"sessionId":"b"}],"pairingLinks":[]}});
    super::fold_access(
        &mut state,
        json!({"type":"clientUpserted","revision":2,"payload":{"sessionId":"a","label":"renamed"}}),
    );
    assert_eq!(
        state["payload"]["clientSessions"].as_array().unwrap().len(),
        2
    );
    super::fold_access(
        &mut state,
        json!({"type":"clientRemoved","revision":1,"payload":{"sessionId":"b"}}),
    );
    assert_eq!(
        state["payload"]["clientSessions"].as_array().unwrap().len(),
        2
    );
    super::fold_access(
        &mut state,
        json!({"type":"clientRemoved","revision":3,"payload":{"sessionId":"b"}}),
    );
    assert_eq!(
        state["payload"]["clientSessions"],
        json!([{"sessionId":"a","label":"renamed"}])
    );
}

fn offline() -> (Net, mpsc::Sender<Incoming>, mpsc::Receiver<Command>) {
    let (tx, commands) = mpsc::channel();
    let (incoming, rx) = mpsc::channel();
    let net = Net {
        state: State::default(),
        tx,
        rx,
        replies: HashMap::new(),
        chunks: HashMap::new(),
        resources: HashMap::new(),
        resource_events: HashMap::new(),
        base_url: String::new(),
        bearer: String::new(),
        incoming_tx: incoming.clone(),
        wake: Arc::new(|| {}),
    };
    (net, incoming, commands)
}

#[test]
fn replies_from_any_environment_reach_the_client_on_screen() {
    let (mut remote, incoming, _commands) = offline();
    let (mut shown, _, _keep) = offline();
    let asked = remote.request("projects.readFile", json!({}));
    assert_ne!(shown.request("projects.readFile", json!({})), asked, "ids never collide across clients");
    incoming.send(Incoming::Reply(asked, Ok(json!("contents")))).unwrap();
    incoming.send(Incoming::Chunk(asked, vec![json!(1)])).unwrap();
    remote.drain();
    remote.hand_off(&mut shown);
    assert_eq!(shown.take_chunks(asked), vec![json!(1)]);
    assert_eq!(shown.take_reply(asked), Some(Ok(json!("contents"))));
    assert_eq!(remote.take_reply(asked), None);
}

#[test]
fn shell_items_bump_the_revision_merged_lists_watch() {
    let mut state = State::default();
    state.fold(Route::Shell, json!({"kind":"project.updated","project":{"id":"p"}}));
    state.fold(Route::Shell, json!({"kind":"synchronized"}));
    assert_eq!(state.shell_revision, 2);
}

#[test]
fn terminal_stream_preserves_bursts_and_discards_unwatched_events() {
    let (mut net, incoming, _commands) = offline();
    let payload = json!({"threadId":"t","terminalId":"term-1"});
    net.watch_resource("terminal.attach", payload.clone());
    let route = Route::Resource("terminal.attach".into(), payload.to_string());
    let events = vec![
        json!({"type":"snapshot","snapshot":{"history":"$ "}}),
        json!({"type":"output","sequence":1,"data":"first"}),
        json!({"type":"output","sequence":2,"data":"second"}),
    ];
    for event in &events {
        incoming
            .send(Incoming::Item(route.clone(), event.clone()))
            .unwrap();
    }
    assert!(net.drain());
    assert_eq!(
        net.take_resource_events("terminal.attach", payload.clone()),
        events
    );
    assert!(
        net.take_resource_events("terminal.attach", payload.clone())
            .is_empty()
    );
    incoming
        .send(Incoming::Item(route, events[1].clone()))
        .unwrap();
    net.drain();
    net.unwatch_resource("terminal.attach", payload.clone());
    assert!(
        net.take_resource_events("terminal.attach", payload)
            .is_empty()
    );
}

#[test]
fn bounded_snapshot_pages_in_earlier_history() {
    let row = |id: &str, ordinal: i32| json!({"visibility":"local","sourceThreadId":"t","sourceItemId":id,"position":0,"item":item(id, ordinal)});
    let mut s = State::default();
    s.thread_views.insert("t".into(), ThreadView::default());
    let mut p = projection();
    p["turnItems"] = json!([item("c", 3), item("hidden", 0)]);
    p["visibleTurnItems"] = json!([row("c", 3)]);
    s.fold(Route::Thread("t".into()), json!({"kind":"snapshot","snapshotSequence":5,"projection":p,"historyCursor":"k1","hasMoreHistory":true}));
    assert_eq!(s.thread_views["t"].history_cursor.as_deref(), Some("k1"));
    assert!(s.thread_views["t"].has_more_history);

    let mut held = row("c", 3);
    held["item"]["output"] = json!("stale page copy");
    let page = json!({"snapshotSequence":5,"items":[row("hidden", 0), row("a", 1), row("b", 2), row("a", 1), held],"nextCursor":"k2","hasMoreHistory":true});
    // A reply for a cursor the view moved past changes nothing.
    s.merge_history("t", "k0", &page);
    assert_eq!(s.thread_views["t"].projection["visibleTurnItems"].as_array().unwrap().len(), 1);

    s.merge_history("t", "k1", &page);
    let view = &s.thread_views["t"];
    let ids: Vec<_> = view.projection["visibleTurnItems"].as_array().unwrap().iter().map(|r| r["sourceItemId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a", "b", "c"]);
    let positions: Vec<_> = view.projection["visibleTurnItems"].as_array().unwrap().iter().map(|r| r["position"].as_u64().unwrap()).collect();
    assert_eq!(positions, [0, 1, 2]);
    assert_eq!(view.projection["visibleTurnItems"][2]["item"]["output"], "done");
    let held_ids: Vec<_> = view.projection["turnItems"].as_array().unwrap().iter().map(|i| i["id"].as_str().unwrap()).collect();
    assert_eq!(held_ids, ["c", "hidden", "a", "b"]);
    assert_eq!(view.history_cursor.as_deref(), Some("k2"));

    // Later events update a paged-in item in place.
    let mut p = view.projection.clone();
    let mut update = item("a", 1);
    update["output"] = json!("edited");
    event(&mut p, "turn-item.updated", update);
    assert_eq!(p["visibleTurnItems"][0]["item"]["output"], "edited");

    // A reconnect's fresh window keeps the rows already shown ahead of it, and their cursor.
    let mut fresh = projection();
    fresh["turnItems"] = json!([item("c", 3), item("d", 4)]);
    fresh["visibleTurnItems"] = json!([row("c", 3), row("d", 4)]);
    s.fold(Route::Thread("t".into()), json!({"kind":"snapshot","snapshotSequence":9,"projection":fresh,"historyCursor":"k9","hasMoreHistory":true}));
    let view = &s.thread_views["t"];
    let ids: Vec<_> = view.projection["visibleTurnItems"].as_array().unwrap().iter().map(|r| r["sourceItemId"].as_str().unwrap()).collect();
    assert_eq!(ids, ["a", "b", "c", "d"]);
    assert_eq!(view.history_cursor.as_deref(), Some("k2"));
    assert_eq!(view.sequence, 9);

    // A window with nothing in common leaves a gap, so it replaces what was shown.
    let mut later = projection();
    later["visibleTurnItems"] = json!([row("z", 40)]);
    s.fold(Route::Thread("t".into()), json!({"kind":"snapshot","snapshotSequence":50,"projection":later,"historyCursor":"k50","hasMoreHistory":true}));
    assert_eq!(s.thread_views["t"].projection["visibleTurnItems"].as_array().unwrap().len(), 1);
    assert_eq!(s.thread_views["t"].history_cursor.as_deref(), Some("k50"));

    s.merge_history("t", "k50", &json!({"snapshotSequence":50,"items":[],"nextCursor":null,"hasMoreHistory":false}));
    assert!(!s.thread_views["t"].has_more_history);
    assert_eq!(s.thread_views["t"].history_cursor, None);
}

#[test]
fn deferred_work_pages_fill_in_behind_their_anchor() {
    let row = |id: &str, ordinal: i32| json!({"visibility":"local","sourceThreadId":"t","sourceItemId":id,"position":0,"item":item(id, ordinal)});
    let mut s = State::default();
    s.thread_views.insert("t".into(), ThreadView::default());
    let mut anchor = row("w1", 2);
    anchor["item"]["deferredWork"] = json!({"cursor":"c1","itemCount":4,"lastOrdinal":5});
    let mut p = projection();
    p["turnItems"] = json!([item("u", 1), anchor["item"].clone(), item("reply", 6)]);
    p["visibleTurnItems"] = json!([row("u", 1), anchor, row("reply", 6)]);
    s.fold(Route::Thread("t".into()), json!({"kind":"snapshot","snapshotSequence":5,"projection":p}));
    assert_eq!(s.work_cursor("t", "w1"), Some("c1"));
    assert_eq!(s.work_cursor("t", "u"), None);

    let page = |rows: &[(&str, i32)], next: Value| json!({"items": rows.iter().map(|&(id, o)| row(id, o)).collect::<Vec<_>>(), "nextCursor": next});
    let ids = |s: &State| -> Vec<String> {
        s.thread_views["t"].projection["visibleTurnItems"].as_array().unwrap().iter().map(|r| r["sourceItemId"].as_str().unwrap().to_owned()).collect()
    };
    assert_eq!(s.merge_work("t", "w1", "c0", &page(&[("w2", 3)], json!("c2"))), Err("Work history changed. Retry loading it.".to_owned()));
    assert_eq!(ids(&s), ["u", "w1", "reply"]);

    // Rows outside the anchor's turn stay out; the anchor's own row keeps its place.
    let first = page(&[("w1", 2), ("w2", 3), ("w3", 4), ("u", 1), ("late", 9)], json!("c2"));
    assert_eq!(s.merge_work("t", "w1", "c1", &first), Ok(true));
    assert_eq!(ids(&s), ["u", "w1", "w2", "w3", "reply"]);
    assert_eq!(s.work_cursor("t", "w1"), Some("c2"));
    assert_eq!(s.merge_work("t", "w1", "c2", &page(&[("w2", 3)], json!("c2"))), Err("Work history did not advance.".to_owned()));

    assert_eq!(s.merge_work("t", "w1", "c2", &page(&[("w4", 5)], Value::Null)), Ok(false));
    assert_eq!(ids(&s), ["u", "w1", "w2", "w3", "w4", "reply"]);
    {
        let p = &s.thread_views["t"].projection;
        assert_eq!(p["visibleTurnItems"][5]["position"], 5);
        assert!(p["visibleTurnItems"][1]["item"].get("deferredWork").is_none());
        let held = p["turnItems"].as_array().unwrap();
        assert!(held.iter().find(|i| i["id"] == "w1").is_some_and(|i| i.get("deferredWork").is_none()));
        assert!(held.iter().any(|i| i["id"] == "w4"));
    }
    // Once nothing is deferred, a late reply changes nothing.
    assert_eq!(s.merge_work("t", "w1", "c2", &page(&[("x", 3)], Value::Null)), Ok(false));
    assert_eq!(ids(&s).len(), 6);
}
