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
