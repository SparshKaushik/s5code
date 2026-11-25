use super::rows::{self, Row};
use crate::{App, Appearance, Route};

fn thread(app: &mut App, title: &str) {
    let id = app
        .state()
        .thread_views
        .iter()
        .find(|(_, v)| crate::data::s(&v.projection["thread"], "title") == title)
        .map(|(id, _)| id.clone());
    if let Some(id) = id {
        app.navigate(Route::Thread(id));
    }
    crate::scene_clock(app);
}
fn showcase(app: &mut App) {
    thread(app, "Replay showcase");
}
fn todo(app: &mut App) {
    thread(app, "Todo list replay");
}
fn approval(app: &mut App) {
    thread(app, "Approval flow replay");
}
fn idempotency(app: &mut App) {
    thread(app, "Idempotency key definition");
}
fn sidebar(app: &mut App) {
    thread(app, "Sidebar v2 pinned and active sections");
}
fn docs(app: &mut App) {
    thread(app, "Docs site navigation ideas");
}
fn expanded(app: &mut App) {
    showcase(app);
    if let Route::Thread(id) = &app.route {
        let ids = app
            .state()
            .thread_views
            .get(id)
            .map(|v| {
                rows::derive(&v.projection, app.now_ms)
                    .iter()
                    .filter_map(|r| {
                        if let Row::Work { id, .. } = r {
                            Some(id.clone())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        app.ui.chat.timeline.expanded.extend(ids);
    }
}
fn pending(app: &mut App) {
    thread(app, "Timeline approval fixture");
}
fn pending_warning(app: &mut App) {
    pending(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(view) = state.thread_views.get_mut(&id)
    {
        let options = serde_json::json!([
            {"decision": "decline", "label": "Decline"},
            {"decision": "accept", "label": "Approve", "warning": "This runs outside the sandbox with full disk access."},
            {"decision": "acceptForSession", "label": "Always allow this session"}]);
        for list in ["visibleTurnItems", "turnItems"] {
            for entry in view.projection[list].as_array_mut().into_iter().flatten() {
                let item = if entry["item"].is_object() { &mut entry["item"] } else { entry };
                if item["type"] == "approval_request" {
                    item["options"] = options.clone();
                }
            }
        }
    }
}
fn question(app: &mut App) {
    thread(app, "Timeline question fixture");
}
fn working(app: &mut App) {
    thread(app, "Timeline showcase fixture");
}
fn error(app: &mut App) {
    showcase(app);
    if let Route::Thread(id) = &app.route {
        if let crate::data::Source::Fixture(state) = &mut app.src {
            if let Some(v) = state.thread_views.get_mut(id) {
                if let Some(items) = v.projection["visibleTurnItems"].as_array_mut() {
                    items.push(serde_json::json!({"position":100,"item":{"id":"scene-error","type":"error","status":"failed","failure":{"class":"transport_error","message":"The provider process exited unexpectedly. Please try again.","code":null,"retryable":true},"retry":{"attempt":1,"maxAttempts":3,"retryDelayMs":1000}}}));
                }
            }
        }
    }
}
/// The showcase thread with a failed latest run whose root error is `failure`.
fn failed_run(app: &mut App, failure: serde_json::Value) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        let p = &mut v.projection;
        let run = serde_json::json!({"id": "scene-run", "ordinal": 999, "status": "failed", "rootNodeId": "scene-root",
            "startedAt": "2026-12-01T00:00:00.000Z", "completedAt": "2026-12-01T00:00:01.000Z"});
        let item = serde_json::json!({"id": "scene-failure", "type": "error", "status": "failed", "runId": "scene-run", "nodeId": "scene-root",
            "ordinal": 1, "updatedAt": "2026-12-01T00:00:01.000Z", "failure": failure});
        for (key, value) in [("runs", run), ("turnItems", item)] {
            match p[key].as_array_mut() {
                Some(list) => list.push(value),
                None => p[key] = serde_json::json!([value]),
            }
        }
    }
}
fn error_banner(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "provider_error", "code": null, "retryable": false,
        "message": "{\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded: the provider is temporarily unable to take requests.\"},\"request_id\":\"req_011CScene\"}"}));
}
fn error_banner_chatgpt(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "provider_error", "retryable": false,
        "code": "subscription_sharing_workplace_individual_usage_limit_exceeded", "message": "You've hit your usage limit."}));
}
fn system_notification(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, (kind, summary)) in [("system", "T3 Code restarted and resumed this turn"), ("command", "Background command finished")].into_iter().enumerate() {
            items.push(serde_json::json!({"position": 100 + n, "item": {"id": format!("scene-notification-{kind}"), "type": "notification",
                "status": "completed", "outcome": "updated", "source": {"kind": kind}, "summary": summary, "detail": "Continue",
                "createdAt": "2026-01-01T00:00:00Z"}}));
        }
    }
}
fn chip_details(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let records = crate::chat::composer::chips::samples();
    let link = |r: &serde_json::Value| format!("[{}](t3-context://v1/{}/{})", crate::data::s(r, "label"), crate::data::s(r, "kind"), crate::data::s(r, "contextId"));
    let text = format!("Why does {} fail? See {}", link(&records[0]), link(&records[2]));
    let open = crate::data::s(&records[0], "contextId").to_owned();
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        items.push(serde_json::json!({"position": 100, "item": {"id": "scene-chips", "type": "user_message", "text": text,
            "createdAt": "2026-01-01T00:00:00Z", "attachments": [], "context": {"records": records}}}));
    }
    app.ui.chat.timeline.chip = Some(("scene-chips".into(), open));
}
fn long_message(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let text = (1..=20).map(|n| format!("Step {n}: check the {n}th migration and note anything odd.")).collect::<Vec<_>>().join("\n");
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        items.push(serde_json::json!({"position": 100, "item": {"id": "scene-long", "type": "user_message", "text": text,
            "createdAt": "2026-01-01T00:00:00Z", "attachments": []}}));
        items.push(serde_json::json!({"position": 101, "item": {"id": "scene-failed-tool", "type": "command_execution",
            "status": "failed", "input": "pnpm test --filter migrations", "createdAt": "2026-01-01T00:00:01Z"}}));
    }
}
fn secret_request(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, (status, label)) in [("saved", "OPENAI_API_KEY"), ("pending", "STRIPE_WEBHOOK_SECRET")].into_iter().enumerate() {
            items.push(serde_json::json!({"position": 100 + n, "visibility": "local", "item": {"id": format!("scene-secret-{n}"),
                "type": "secret_request", "threadId": id, "status": "waiting", "secretStatus": status, "label": label,
                "reason": "The webhook handler verifies signatures with it.", "createdAt": "2026-01-01T00:00:00Z"}}));
        }
    }
}
fn long_message_open(app: &mut App) {
    long_message(app);
    app.ui.chat.timeline.expanded.insert("user-full:scene-long".into());
}
fn light(app: &mut App) {
    showcase(app);
    app.appearance = Appearance::Light;
}
pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("timeline-pending-approval", pending),
    ("timeline-question", question),
    ("timeline-error", error),
    ("timeline-error-banner", error_banner),
    ("timeline-error-banner-chatgpt", error_banner_chatgpt),
    ("timeline-system-notification", system_notification),
    ("timeline-working", working),
    ("timeline-showcase", showcase),
    ("timeline-todo", todo),
    ("timeline-approval", approval),
    ("timeline-opencode-idempotency", idempotency),
    ("timeline-opencode-sidebar", sidebar),
    ("timeline-opencode-docs", docs),
    ("timeline-expanded", expanded),
    ("timeline-light", light),
    ("timeline-chip-details", chip_details),
    ("timeline-long-message", long_message),
    ("timeline-long-message-open", long_message_open),
    ("timeline-approval-warning", pending_warning),
    ("timeline-secret-request", secret_request),
    ("timeline-link-request", link_request),
    ("timeline-changed-expand-all", changed_expand_all),
    ("timeline-provider-banner", provider_banner),
    ("timeline-usage-limit-banner", usage_limit_banner),
    ("timeline-child-approval", child_approval),
    ("timeline-worktree-setup", worktree_setup),
    ("timeline-worktree-handed-off", worktree_handed_off),
    ("timeline-worktree-retry", worktree_retry),
    ("timeline-tool-group", tool_group),
    ("timeline-tool-group-open", tool_group_open),
    ("timeline-tool-live", tool_live),
    ("timeline-thinking", thinking),
    ("timeline-tool-inspector", tool_inspector),
    ("timeline-link-chips", link_chips),
    ("chat-server-update-running", server_update_running),
    ("chat-server-update-failed", server_update_failed),
    ("timeline-user-markdown", user_markdown),
    ("timeline-skill-chips", skill_chips),
    ("timeline-steer", steer),
    ("timeline-load-earlier", load_earlier),
    ("timeline-load-earlier-error", load_earlier_error),
    ("timeline-answered-questions", answered_questions),
    ("timeline-now", thread_now),
    ("timeline-now-stop", thread_now_stop),
    ("timeline-header-subagent", header_subagent),
    ("timeline-draft-hero", draft_hero),
    ("timeline-draft-hero-empty", draft_hero_empty),
    ("chat-triage-hero", crate::chat::home::triage_scene),
    ("timeline-work-history-loading", work_history_loading),
    ("timeline-work-history-error", work_history_error),
    ("timeline-command-output", command_output),
    ("timeline-provider-errors", provider_errors),
    ("timeline-lifecycle", lifecycle_rows),
    ("timeline-parent-divider", parent_divider),
    ("timeline-user-attribution", user_attribution),
    ("timeline-steer-segments", steer_segments),
    ("timeline-steer-segments-open", steer_segments_open),
    ("timeline-superseded-attempt", superseded_attempt),
    ("timeline-superseded-attempt-open", superseded_attempt_open),
    ("timeline-subagent-group", subagent_group),
    ("timeline-subagent-group-open", subagent_group_open),
    ("timeline-compacting", compacting),
    ("chat-side-pane", side_pane),
    ("chat-side-pane-composer", side_pane_composer),
    ("chat-side-pane-stop", side_pane_stop),
    ("chat-side-reopen", side_reopen),
    ("chat-side-details-list", side_details_list),
    ("chat-side-composer-banners", side_composer_question),
    ("chat-side-composer-plan", side_composer_plan),
    ("timeline-work-icons", work_icons),
    ("timeline-interrupted-meta", interrupted_meta),
    ("timeline-attempt-nested", attempt_nested),
    ("timeline-attempt-nested-open", attempt_nested_open),
    ("timeline-subagent-card", subagent_card),
    ("details-items-menu", crate::chat::details::items::scene),
    ("details-items-empty", crate::chat::details::items::scene_empty),
    ("details-servers", crate::chat::details::servers::scene),
    ("details-automations", crate::chat::details::automations::scene),
    ("dialog-native-child-work", crate::chat::details::child_work::scene),
    ("composer-banner-feedback", composer_banner_feedback),
    ("composer-banner-fallback", composer_banner_fallback),
    ("composer-banner-runtime-ceiling", composer_banner_runtime_ceiling),
    ("composer-banner-stack", composer_banner_stack),
    ("timeline-now-banners", timeline_now_banners),
    ("timeline-citations", citations),
    ("timeline-citations-with-chips", citations_with_chips),
    ("composer-citation-comment", crate::chat::composer::chips::comment_scene),
    ("timeline-media-actions", media_actions),
    ("timeline-run-in-terminal", run_in_terminal),
    ("timeline-video-fallback", video_fallback),
    ("timeline-stopped-replies", stopped_replies),
    ("timeline-html-render", html_render),
    ("timeline-mcp-app", mcp_app),
    ("timeline-html-render-chrome", html_render_chrome),
    ("timeline-mcp-app-chrome", mcp_app_chrome),
    ("timeline-subagent-remote", subagent_remote),
    ("composer-subagent-bar", subagent_bar),
    ("composer-subagent-bar-instance", subagent_bar_instance),
    ("composer-usage-limits-reset-credits", usage_reset_credits),
    ("timeline-workflow-opening", workflow_opening),
    ("timeline-minimap", timeline_minimap),
    ("timeline-markdown-alerts", markdown_alerts),
    ("timeline-codex-directives", codex_directives),
    ("timeline-file-card", file_card),
    ("timeline-lightbox-nav", lightbox_nav),
    ("timeline-optimistic-send", optimistic_send),
    ("chat-drop-overlay", drop_overlay),
    ("timeline-offline-reconnecting", offline_reconnecting),
    ("timeline-offline-disconnect", offline_disconnect),
    ("timeline-guest-share", guest_share),
    ("timeline-lightbox-zoom", lightbox_zoom),
    ("timeline-minimap-in-view", minimap_in_view),
    ("chat-width-normal", |app| chat_width(app, "normal")),
    ("composer-context-strip", context_strip),
    ("chat-width-wide", |app| chat_width(app, "wide")),
    ("chat-width-full", |app| chat_width(app, "full")),
    ("timeline-math", math),
    ("timeline-math-typeset", math_typeset),
];

/// The showcase under a Chat width other than Comfortable: the timeline and composer widen.
/// The chat column at each width with the details card closed; render wider than 1440 to tell
/// wide from full.
fn chat_width(app: &mut App, width: &str) {
    showcase(app);
    app.client_settings.values.insert("chatWidth".into(), serde_json::json!(width));
    crate::chat::details::toggle(app, true);
}
/// "Keep composer context visible" on a started thread: the strip stays under the composer.
fn context_strip(app: &mut App) {
    showcase(app);
    app.client_settings.values.insert("persistComposerContextStrip".into(), serde_json::json!(true));
}
/// Render math on: each formula reads as its TeX source; prices stay prose.
fn math(app: &mut App) {
    showcase(app);
    app.client_settings.values.insert("mathRenderingEnabled".into(), serde_json::json!(true));
    let text = "The loss is $L = \\frac{1}{n}\\sum_i (y_i - \\hat y_i)^2$, and \\(n\\) is the batch size. It costs $5 and $10 a run.\n\n$$\n\\nabla L = -\\frac{2}{n} X^T (y - \\hat y)\n$$\n\n```math\ne^{i\\pi} + 1 = 0\n```";
    push_visible(app, vec![serde_json::json!({"id": "scene-math", "type": "assistant_message", "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z"})]);
}

/// The same formulas once KaTeX typeset them: striped stand-ins of their size, since the render
/// host has no Chrome; the inline paragraph lays out a word at a time.
fn math_typeset(app: &mut App) {
    math(app);
    for (tex, display, size) in [
        ("L = \\frac{1}{n}\\sum_i (y_i - \\hat y_i)^2", false, (118.0, 26.0)),
        ("n", false, (10.0, 18.0)),
        ("\\nabla L = -\\frac{2}{n} X^T (y - \\hat y)", true, (210.0, 40.0)),
        ("e^{i\\pi} + 1 = 0", true, (96.0, 22.0)),
    ] {
        super::katex::scene_seed(app, tex, display, size);
    }
}

/// The lightbox's middle picture zoomed 2x and panned right and down.
fn lightbox_zoom(app: &mut App) {
    lightbox_nav(app);
    super::attachments::scene_zoom(app, 2.0, [160.0, 90.0]);
}

/// The minimap with the prompts in view lit (the list rests at the end), the first strip's
/// preview open so the rail shows.
fn minimap_in_view(app: &mut App) {
    timeline_minimap(app);
    app.ui.chat.timeline.minimap.active = Some(0);
}

/// Every GitHub alert kind, and a `<details>` folded and one written open.
fn markdown_alerts(app: &mut App) {
    showcase(app);
    let text = "Before you merge:\n\n> [!NOTE]\n> The cache warms on first request.\n\n> [!TIP]\n> Run `vp test run` while iterating.\n\n> [!IMPORTANT]\n> Migrations run in order.\n\n> [!WARNING]\n> This drops the old index.\n\n> [!CAUTION]\n> Back up `statev2.sqlite` first.\n\n<details>\n<summary>Full test log</summary>\n\n```sh\nvp test run\n```\n</details>\n\n<details open>\n<summary>Why this is safe</summary>\n\nThe index is rebuilt **before** the old one drops.\n</details>";
    push_visible(app, vec![serde_json::json!({"id": "scene-alerts", "type": "assistant_message", "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z"})]);
}
/// A Codex reply citing a file, offering follow-ups, and an artifact template card.
fn codex_directives(app: &mut App) {
    showcase(app);
    let text = "Created :codex-file-citation{path=\"outputs/report.xlsx\" line_range_start=\"4\"}.\n\n::artifact-template{skill_name=\"artifact-template-hello-world\" skill_directory=\"/Users/test/.codex/skills/artifact-template-hello-world\" display_name=\"Hello World\" artifact_kind=\"document\"}\n\nNext steps:\n\n- :codex-followup[Prepare print version]{prompt=\"Prepare the document for printing.\"}\n- :codex-followup[Review **document**]{prompt=\"Review it.\"}";
    push_visible(app, vec![serde_json::json!({"id": "scene-directives", "type": "assistant_message", "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z"})]);
}
/// Sent files: one signed (Preview and Download) and one still unsigned (Download off).
fn file_card(app: &mut App) {
    use serde_json::json;
    showcase(app);
    push_visible(app, vec![json!({"id": "scene-file-card", "type": "user_message", "status": "completed", "inputIntent": "turn_start",
        "text": "Here is the failing build.", "createdAt": "2026-01-01T00:00:00Z", "attachments": [
            {"type": "file", "id": "scene-file-signed", "name": "build.log", "mimeType": "text/plain", "sizeBytes": 48_213},
            {"type": "file", "id": "scene-file-unsigned", "name": "trace.json", "mimeType": "application/json", "sizeBytes": 2048}]})]);
    let signed = super::media::Loaded::new("https://example.com/build.log".into(), "build.log".into(), None, 0, None);
    app.ui.chat.timeline.assets.ready.insert("scene-file-signed".into(), signed);
}
/// The lightbox on the second of three pictures in one paragraph: Previous, Next and "(2/3)".
fn lightbox_nav(app: &mut App) {
    showcase(app);
    let root = super::workspace_root(app).unwrap_or_else(|| "/tmp".into());
    let srcs: Vec<String> = ["before", "after", "diff"].iter().map(|n| format!("{}/docs/{n}.png", root.trim_end_matches('/'))).collect();
    let text = format!("Screens:\n\n![Before]({}) ![After]({}) ![Diff]({})", srcs[0], srcs[1], srcs[2]);
    push_visible(app, vec![serde_json::json!({"id": "scene-gallery", "type": "assistant_message", "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z"})]);
    let keys: Vec<String> = [("Before", &srcs[0]), ("After", &srcs[1]), ("Diff", &srcs[2])].into_iter().filter_map(|(alt, src)| super::media::scene_entry(app, alt, src, false)).collect();
    let assets = &mut app.ui.chat.timeline.assets;
    assets.open = keys.get(1).cloned();
    assets.gallery = keys;
}

/// A prompt quoting the showcase's first reply twice, the first quote's popover open.
fn citations(app: &mut App) {
    use super::citation::{Citation, href};
    showcase(app);
    let Route::Thread(thread) = app.route.clone() else { return };
    let reply = app.state().thread_views.get(&thread).and_then(|v| {
        rows::array(&v.projection, "visibleTurnItems").iter().map(|i| &i["item"]).find(|i| crate::data::s(i, "type") == "assistant_message").cloned()
    });
    let message = reply.as_ref().map(|i| crate::data::s(i, "messageId").to_owned()).filter(|m| !m.is_empty()).unwrap_or_else(|| "scene-message".into());
    let quoted = reply.as_ref().map(|i| crate::data::s(i, "text").chars().take(120).collect::<String>()).filter(|q| !q.trim().is_empty()).unwrap_or_else(|| "Group tool calls under one header.".into());
    let cite = |comment: Option<&str>| Citation {
        environment_id: "local".into(),
        thread_id: thread.clone(),
        message_id: message.clone(),
        text: quoted.clone(),
        comment: comment.map(str::to_owned),
        start: 0,
        end: quoted.encode_utf16().count() as u64,
        prefix: String::new(),
        suffix: String::new(),
    };
    let first = href(&cite(None));
    let text = format!("Compare [Assistant quote]({first}) with the plan.\n\nAnd this: [Assistant quote]({})", href(&cite(Some("Can we keep this but shorter?"))));
    push_visible(app, vec![serde_json::json!({"id": "scene-citations", "type": "user_message", "text": text, "createdAt": "2026-01-01T00:00:00Z", "attachments": []})]);
    app.ui.chat.timeline.md_links.citation = Some(("md-block-scene-citations-0-0".into(), first));
}
/// A prompt with a terminal chip and a commented quote, which read as chips in the bubble's
/// plain text; the quote's popover open.
fn citations_with_chips(app: &mut App) {
    use super::citation::{Citation, href};
    showcase(app);
    let Route::Thread(thread) = app.route.clone() else { return };
    let quoted = "Group tool calls under one header so a long turn reads as one step.";
    let quote = href(&Citation {
        environment_id: "local".into(),
        thread_id: thread,
        message_id: "scene-message".into(),
        text: quoted.into(),
        comment: Some("Can we keep this but shorter?".into()),
        start: 0,
        end: quoted.encode_utf16().count() as u64,
        prefix: String::new(),
        suffix: String::new(),
    });
    let terminal = crate::chat::composer::chips::samples().remove(0);
    let chip = format!("[{}](t3-context://v1/terminal/{})", crate::data::s(&terminal, "label"), crate::data::s(&terminal, "contextId"));
    // Long enough to wrap twice in the bubble, a chip landing near each line's end.
    let text = format!(
        "Why does {chip} fail after the grouped timeline landed? Compare [Assistant quote]({quote}) with the plan we agreed on, \
         then tell me which step regressed and whether {chip} still reproduces once the header collapses."
    );
    push_visible(app, vec![serde_json::json!({"id": "scene-citation-chips", "type": "user_message", "text": text,
        "createdAt": "2026-01-01T00:00:00Z", "attachments": [], "context": {"records": [terminal]}})]);
    app.ui.chat.timeline.md_links.citation = Some((super::chip::bubble_key("scene-citation-chips"), quote));
}
/// A finished reply whose one-line shell fence offers Run in terminal; the multi-line one does not.
fn run_in_terminal(app: &mut App) {
    showcase(app);
    let text = "Run the suite:\n\n```bash\nvp test run src/billing\n```\n\nThen rebuild:\n\n```bash\nvp i\nvp run build\n```\n";
    push_visible(app, vec![serde_json::json!({"id": "scene-run", "type": "assistant_message", "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z"})]);
}
/// A reply with a workspace screenshot, its media actions menu open.
fn media_actions(app: &mut App) {
    showcase(app);
    let root = super::workspace_root(app).unwrap_or_else(|| "/tmp".into());
    let src = format!("{}/docs/timeline.png", root.trim_end_matches('/'));
    push_visible(app, vec![serde_json::json!({"id": "scene-media", "type": "assistant_message", "status": "completed",
        "text": format!("Here is the grouped timeline:\n\n![Grouped timeline]({src})"), "createdAt": "2026-01-01T00:00:00Z"})]);
    if let Some(key) = super::media::scene_entry(app, "Grouped timeline", &src, false) {
        app.ui.chat.timeline.assets.media.menu = Some((key, 420.0, 360.0));
    }
}
/// A Markdown video and a sent video that can't play: web's unavailable cards.
fn video_fallback(app: &mut App) {
    showcase(app);
    let src = "https://example.com/media/demo-recording.mp4";
    push_visible(app, vec![
        serde_json::json!({"id": "scene-video-sent", "type": "user_message", "text": "Here's the bug on video.", "createdAt": "2026-01-01T00:00:00Z",
            "attachments": [{"type": "video", "id": "scene-video-attachment", "name": "bug.mov", "mimeType": "video/quicktime", "sizeBytes": 2_400_000}]}),
        serde_json::json!({"id": "scene-video-reply", "type": "assistant_message", "status": "completed",
            "text": format!("Recorded the fix:\n\n![Demo recording]({src})"), "createdAt": "2026-01-01T00:00:01Z"}),
    ]);
    app.ui.chat.timeline.assets.failed.insert("scene-video-attachment".into());
    super::media::scene_entry(app, "Demo recording", src, true);
}

/// A turn item of `run` that ended at `end` seconds past `at(0)`, with `fields` merged in.
fn scene_item(run: &str, at: &dyn Fn(u32) -> String, id: &str, kind: &str, end: u32, fields: serde_json::Value) -> serde_json::Value {
    let mut item = serde_json::json!({"id": id, "type": kind, "runId": run, "status": "completed", "createdAt": at(end.saturating_sub(1)),
        "completedAt": at(end), "updatedAt": at(end)});
    for (k, v) in fields.as_object().into_iter().flatten() {
        item[k] = v.clone();
    }
    item
}
/// A finished turn with its fold and tool group open: each call behind its own icon and title,
/// read paths named from the workspace folder, and one thought open under "Thought".
fn work_icons(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let root = super::workspace_root(app).unwrap_or_else(|| "/repo".to_owned());
    let at = |s: u32| format!("2026-12-06T00:00:{s:02}.000Z");
    let run = "scene-icons";
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 994, "status": "completed", "startedAt": at(0), "completedAt": at(30)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| scene_item(run, &at, id, kind, end, fields);
    push_visible(app, vec![
        item("scene-icons-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Why does the upload retry twice?", "attachments": []})),
        item("scene-icons-think", "reasoning", 2, json!({"text": "**Tracing the retry**\n\nThe client wraps `send` in `retry()` and the transport retries too."})),
        item("scene-icons-read", "dynamic_tool", 4, json!({"toolName": "Read", "input": {"file_path": format!("{root}/src/upload/client.rs")}})),
        item("scene-icons-grep", "dynamic_tool", 5, json!({"toolName": "Grep", "input": {"pattern": "fn retry", "path": format!("{root}/src")}})),
        item("scene-icons-search", "file_search", 6, json!({"title": "Searched for retry(", "pattern": "retry("})),
        item("scene-icons-web", "web_search", 8, json!({"title": "Searched the web for exponential backoff jitter", "query": "exponential backoff jitter"})),
        item("scene-icons-mcp", "dynamic_tool", 9, json!({"toolName": "mcp__t3__t3_thread_read", "title": "t3_thread_read", "input": {"threadId": "thread:scene"}})),
        item("scene-icons-thought", "reasoning", 11, json!({"text": "The transport's retry is the one to drop."})),
        item("scene-icons-edit", "file_change", 20, json!({"changes": [{"path": format!("{root}/src/upload/transport.rs")}], "fileName": format!("{root}/src/upload/transport.rs")})),
        item("scene-icons-test", "command_execution", 28, json!({"input": "vp test run src/upload", "exitCode": 0})),
        item("scene-icons-reply", "assistant_message", 30, json!({"text": "The transport retried on top of the client; I removed the transport's retry."})),
    ]);
    app.ui.chat.timeline.expanded.extend(["work:scene-icons", "tools:scene-icons-think", "scene-icons-thought"].map(str::to_owned));
}
/// A turn stopped after its reply while its tools still ran: the reply's status, copy, fork and
/// time sit below that work instead of under the reply.
fn interrupted_meta(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-07T00:00:{s:02}.000Z");
    let run = "scene-stopped";
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 993, "status": "interrupted", "startedAt": at(0), "completedAt": at(20)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| scene_item(run, &at, id, kind, end, fields);
    push_visible(app, vec![
        item("scene-stopped-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Find out why the billing test is flaky.", "attachments": []})),
        item("scene-stopped-reply", "assistant_message", 6, json!({"text": "It races the clock in `expires_at`. I'll rerun it to confirm."})),
        item("scene-stopped-read", "dynamic_tool", 8, json!({"toolName": "Read", "input": {"path": "src/billing/expiry.rs"}})),
        item("scene-stopped-test", "command_execution", 18, json!({"input": "vp test run src/billing --repeat 20", "status": "interrupted"})),
        item("scene-stopped-result", "run_interrupt_result", 20, json!({"message": "Stopped by the user."})),
    ]);
}
/// A finished turn a steer cut off: its first attempt's work nests under "Superseded attempt"
/// inside the open "Worked for" fold.
fn attempt_nested(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-08T00:00:{s:02}.000Z");
    let (run, old, new) = ("scene-nested-run", "scene-nested-old-root", "scene-nested-new-root");
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 992, "status": "completed", "rootNodeId": old, "startedAt": at(0), "completedAt": at(40)})]);
    push_items(app, "attempts", vec![
        json!({"id": "scene-nested-1", "runId": run, "attemptOrdinal": 1, "rootNodeId": old, "reason": "initial", "status": "superseded", "completedAt": at(14)}),
        json!({"id": "scene-nested-2", "runId": run, "attemptOrdinal": 2, "rootNodeId": new, "reason": "steering_restart", "status": "completed", "completedAt": at(40)}),
    ]);
    let item = |id: &str, node: &str, kind: &str, end: u32, fields: serde_json::Value| {
        let mut item = scene_item(run, &at, id, kind, end, fields);
        item["nodeId"] = json!(node);
        item
    };
    push_visible(app, vec![
        item("scene-nested-prompt", old, "user_message", 0, json!({"inputIntent": "turn_start", "attachments": [], "text": "Rename the billing tables."})),
        item("scene-nested-think", old, "reasoning", 2, json!({"text": "**Planning the rename**"})),
        item("scene-nested-search", old, "command_execution", 4, json!({"input": "rg -l billing_ src/", "exitCode": 0})),
        item("scene-nested-read", old, "dynamic_tool", 6, json!({"toolName": "Read", "input": {"path": "src/db/schema.rs"}})),
        item("scene-nested-cut", old, "assistant_message", 9, json!({"status": "interrupted", "text": "I'll start with the schema, then"})),
        item("scene-nested-steer", new, "user_message", 12, json!({"inputIntent": "steer", "attachments": [], "text": "Keep the old table names as views."})),
        item("scene-nested-test", new, "command_execution", 30, json!({"input": "vp test run src/db", "exitCode": 0})),
        item("scene-nested-reply", new, "assistant_message", 40, json!({"text": "Renamed the tables and kept the old names as views."})),
    ]);
    app.ui.chat.timeline.expanded.insert(format!("work:{run}"));
}
fn attempt_nested_open(app: &mut App) {
    attempt_nested(app);
    app.ui.chat.timeline.expanded.insert("attempt-fold:scene-nested-1".into());
}
/// Delegated tasks' notices as their tasks' cards: finished, failed and stopped with the time
/// each came, and one whose task this thread doesn't know, as a plain row.
fn subagent_card(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let child = other_thread(app);
    let at = |s: u32| format!("2026-12-09T09:41:{s:02}.000Z");
    let task = |id: &str, driver: &str, status: &str, title: &str, done: u32, fields: serde_json::Value| {
        let mut agent = json!({"id": id, "childThreadId": child, "driver": driver, "status": status, "title": title,
            "startedAt": at(0), "completedAt": at(done), "updatedAt": at(done), "result": null});
        for (k, v) in fields.as_object().into_iter().flatten() {
            agent[k] = v.clone();
        }
        agent
    };
    push_items(app, "subagents", vec![
        task("scene-task-review", "codex", "completed", "Review the schema diff", 40, json!({"result": "No blocking issues; two naming nits in the views."})),
        task("scene-task-tests", "claudeAgent", "failed", "Run the migration tests", 50, json!({"result": "3 tests failed in billing_migrations."})),
        task("scene-task-docs", "opencode", "cancelled", "Check the docs", 30, json!({"progress": "Reading docs/billing.md"})),
    ]);
    let notice = |id: &str, outcome: &str, source: serde_json::Value, end: u32, summary: &str| {
        json!({"id": id, "type": "notification", "status": "completed", "outcome": outcome, "source": source, "summary": summary, "createdAt": at(end)})
    };
    let delegated = |task: &str| json!({"kind": "delegated_task", "childThreadId": child, "taskIds": [task]});
    push_visible(app, vec![
        notice("scene-notice-review", "completed", delegated("scene-task-review"), 41, "Review the schema diff finished"),
        notice("scene-notice-tests", "failed", delegated("scene-task-tests"), 51, "Run the migration tests failed"),
        notice("scene-notice-docs", "cancelled", delegated("scene-task-docs"), 31, "Check the docs stopped"),
        notice("scene-notice-unknown", "completed", json!({"kind": "subagent", "childThreadId": "scene-unknown-child"}), 55, "Audit finished"),
    ]);
}

/// Appends `value` to the route thread's projection `list`, creating the list when missing.
fn push_items(app: &mut App, list: &str, values: Vec<serde_json::Value>) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        match v.projection[list].as_array_mut() {
            Some(items) => items.extend(values),
            None => v.projection[list] = serde_json::Value::Array(values),
        }
    }
}
/// Sets fields on the route thread's sidebar row.
fn patch_shell(app: &mut App, fields: serde_json::Value) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(row) = state.threads.iter_mut().find(|r| crate::data::s(r, "id") == id)
    {
        for (k, v) in fields.as_object().into_iter().flatten() {
            row[k] = v.clone();
        }
    }
}
fn link_request(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let pending = serde_json::json!({"id": "scene-link", "type": "link_request", "status": "waiting", "linkStatus": "pending",
        "threadId": id, "label": "Build box", "url": "https://build.example.ts.net", "hint": "the build box",
        "requestedAccess": "read-only", "reason": "The GPU tests only run on the build box.", "createdAt": "2026-01-01T00:00:00Z"});
    let linked = serde_json::json!({"id": "scene-link-done", "type": "link_request", "status": "completed", "linkStatus": "linked",
        "threadId": id, "linkedLabel": "Studio", "linkedAccess": "full-access", "createdAt": "2026-01-01T00:00:00Z"});
    push_items(app, "visibleTurnItems", vec![
        serde_json::json!({"position": 100, "item": linked}),
        serde_json::json!({"position": 101, "item": pending.clone()}),
    ]);
    super::link::open(app, &pending);
}
fn changed_expand_all(app: &mut App) {
    showcase(app);
    let reply = serde_json::json!({"id": "scene-changed-reply", "type": "assistant_message", "runId": "scene-changed-run",
        "text": "Done. The changed-files card can now expand every folder at once.", "createdAt": "2026-01-01T00:00:00Z"});
    push_items(app, "turnItems", vec![reply.clone()]);
    push_items(app, "visibleTurnItems", vec![serde_json::json!({"position": 100, "item": reply})]);
    push_items(app, "checkpoints", vec![serde_json::json!({"runId": "scene-changed-run", "appRunOrdinal": 99, "status": "ready", "files": [
        {"path": "src/chat/timeline/changed.rs", "additions": 42, "deletions": 6},
        {"path": "src/chat/timeline/link.rs", "additions": 380, "deletions": 0},
        {"path": "src/shell/sidebar/mod.rs", "additions": 3, "deletions": 1},
        {"path": "README.md", "additions": 2, "deletions": 0}]})]);
    app.ui.chat.timeline.expanded.insert("changed-all:scene-changed-run".into());
}
fn provider_banner(app: &mut App) {
    showcase(app);
    let Some((instance, _)) = crate::chat::composer::selection(app) else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(p) = state.config.as_mut().and_then(|c| c["providers"].as_array_mut()).and_then(|ps| ps.iter_mut().find(|p| crate::data::s(p, "instanceId") == instance))
    {
        p["status"] = "error".into();
        p["installed"] = true.into();
        p["auth"] = serde_json::json!({"status": "unauthenticated"});
    }
}
fn usage_limit_banner(app: &mut App) {
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    patch_shell(app, serde_json::json!({"status": "failed", "lastErrorClass": "usage_limit", "latestRunId": "scene-run",
        "latestRunCompletedAt": "2026-03-10T11:50:00Z", "usageLimitResetAt": "2026-03-10T15:30:00Z",
        "limitRecovery": {"runId": "scene-run", "resetAt": "2026-03-10T15:30:00Z", "autoResume": true}}));
}
fn child_approval(app: &mut App) {
    let approval = app.state().thread_views.iter().find(|(_, v)| crate::data::s(&v.projection["thread"], "title") == "Timeline approval fixture").map(|(_, v)| v.projection.clone());
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    let Some(projection) = approval else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-child", "title": "Migrate the billing schema", "projectId": project,
            "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
            "pendingRuntimeRequest": {"kind": "command"}}));
        state.thread_views.insert("scene-child".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
    }
}
/// A setup script printing while the agent waits to start.
fn worktree_setup(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:42Z").unwrap_or(app.now_ms);
    let snapshot = serde_json::json!({"threadId": id, "sequence": 7, "phase": "running", "startedAt": "2026-03-10T12:00:00Z",
        "branch": "t3/billing-schema", "baseRef": "origin/main", "worktreePath": "/home/dev/.t3/worktrees/app/billing-schema",
        "setupScript": {"name": "Install dependencies", "command": "pnpm install --frozen-lockfile", "terminalId": "setup-1"},
        "stages": [
            {"id": "fetch", "status": "done", "startedAt": "2026-03-10T12:00:00Z", "endedAt": "2026-03-10T12:00:03Z"},
            {"id": "checkout", "status": "done", "startedAt": "2026-03-10T12:00:03Z", "endedAt": "2026-03-10T12:00:09Z"},
            {"id": "submodules", "status": "skipped", "detail": "none"},
            {"id": "setup-script", "status": "running", "startedAt": "2026-03-10T12:00:09Z",
                "tail": ["Lockfile is up to date", "Progress: resolved 1412, reused 1398, downloaded 14", "Packages: +1412", "node_modules/.pnpm/esbuild postinstall…"]},
            {"id": "agent", "status": "pending"}]});
    app.ui.chat.timeline.worktree.details.insert(id.clone());
    app.ui.chat.timeline.worktree.held.insert(id, snapshot);
}
/// The agent started from a worktree whose setup script failed: the card is one summary row.
fn worktree_handed_off(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    patch_shell(app, serde_json::json!({"latestRunStartedAt": "2026-03-10T12:00:31Z"}));
    // A follow-up would retire the card, so the thread keeps only its first prompt.
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(items) = state.thread_views.get_mut(&id).and_then(|v| v.projection["turnItems"].as_array_mut())
    {
        let mut prompts = 0;
        items.retain(|i| i["type"] != "user_message" || { prompts += 1; prompts == 1 });
    }
    let snapshot = serde_json::json!({"threadId": id, "sequence": 9, "phase": "done", "startedAt": "2026-03-10T12:00:00Z", "endedAt": "2026-03-10T12:00:31Z",
        "branch": "t3/billing-schema", "baseRef": "origin/main", "worktreePath": "/home/dev/.t3/worktrees/app/billing-schema",
        "setupScript": {"name": "Install dependencies", "command": "pnpm install --frozen-lockfile", "terminalId": "setup-1"},
        "stages": [
            {"id": "fetch", "status": "done", "startedAt": "2026-03-10T12:00:00Z", "endedAt": "2026-03-10T12:00:03Z"},
            {"id": "checkout", "status": "done", "startedAt": "2026-03-10T12:00:03Z", "endedAt": "2026-03-10T12:00:09Z"},
            {"id": "submodules", "status": "skipped", "detail": "none"},
            {"id": "setup-script", "status": "failed", "startedAt": "2026-03-10T12:00:09Z", "endedAt": "2026-03-10T12:00:30Z", "tail": ["ERR_PNPM_LOCKFILE_MISSING"]},
            {"id": "agent", "status": "done", "startedAt": "2026-03-10T12:00:30Z", "endedAt": "2026-03-10T12:00:31Z"}]});
    app.ui.chat.timeline.worktree.details.insert(id.clone());
    app.ui.chat.timeline.worktree.held.insert(id, snapshot);
}
/// A new thread's first message waiting on its worktree, so it error row.
fn worktree_retry(app: &mut App) {
    failed_run(app, serde_json::json!({"class": "workspace_error", "code": "workspace_preparation_failed", "retryable": true,
        "message": "git worktree add failed: branch t3/billing-schema is already checked out"}));
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        let p = &mut v.projection;
        for run in p["runs"].as_array_mut().into_iter().flatten() {
            if run["id"] == "scene-run" {
                run["workspacePreparation"] = serde_json::json!({"status": "failed"});
            }
        }
        let item = super::rows::array(p, "turnItems").iter().find(|i| i["id"] == "scene-failure").cloned();
        if let Some(items) = p["visibleTurnItems"].as_array_mut()
            && let Some(item) = item
        {
            items.push(serde_json::json!({"position": 100, "item": item}));
        }
    }
}

/// Appends `items` to the route thread's visible items, from position 100 on.
fn push_visible(app: &mut App, items: Vec<serde_json::Value>) {
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(list) = state.thread_views.get_mut(&id).and_then(|v| v.projection["visibleTurnItems"].as_array_mut())
    {
        for (n, item) in items.into_iter().enumerate() {
            list.push(serde_json::json!({"position": 100 + n, "item": item}));
        }
    }
}
/// The showcase thread with a turn in flight: a prompt and five tool calls, the last still
/// running when `busy`. The clock reads 47s into the turn, 7s after its last step.
fn live_turn(app: &mut App, busy: bool) {
    use serde_json::json;
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let run = json!({"id": "scene-live", "ordinal": 998, "status": "running", "rootNodeId": "scene-live-root", "startedAt": "2026-12-01T00:00:00.000Z"});
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        match v.projection["runs"].as_array_mut() {
            Some(runs) => runs.push(run),
            None => v.projection["runs"] = json!([run]),
        }
    }
    let at = |s: u32| format!("2026-12-01T00:00:{s:02}.000Z");
    let done = |mut item: serde_json::Value, from: u32, to: u32| {
        item["runId"] = json!("scene-live");
        item["status"] = json!("completed");
        item["createdAt"] = json!(at(from));
        item["completedAt"] = json!(at(to));
        item["updatedAt"] = json!(at(to));
        item
    };
    let mut test = done(
        json!({"id": "scene-tool-test", "type": "command_execution", "input": "vp test run src/chat/timeline", "exitCode": 0,
            "output": "✓ rows.rs (14 tests)\n✓ work.rs (6 tests)\n\nTest Files  2 passed (2)\n     Tests  20 passed (20)"}),
        31,
        40,
    );
    if busy {
        test["status"] = json!("running");
        test["output"] = json!("✓ rows.rs (14 tests)");
        if let Some(fields) = test.as_object_mut() {
            fields.remove("exitCode");
            fields.remove("completedAt");
        }
    }
    push_visible(app, vec![
        json!({"id": "scene-live-prompt", "type": "user_message", "runId": "scene-live", "inputIntent": "turn_start", "status": "completed",
            "text": "Run the timeline tests and fix whatever breaks.", "createdAt": at(0), "attachments": []}),
        done(json!({"id": "scene-tool-read-a", "type": "dynamic_tool", "toolName": "Read", "title": "Read rows.rs",
            "input": {"path": "src/chat/timeline/rows.rs"}}), 2, 3),
        done(json!({"id": "scene-tool-read-b", "type": "dynamic_tool", "toolName": "Read", "title": "Read work.rs",
            "input": {"path": "src/chat/timeline/work.rs", "limit": 200}}), 3, 4),
        done(json!({"id": "scene-tool-search", "type": "file_search", "title": "Searched for fn derive", "pattern": "fn derive",
            "results": [{"fileName": "src/chat/timeline/rows.rs", "line": 124, "preview": "pub fn derive(p: &Value, now: i64) -> Vec<Row<'_>> {"}]}), 5, 6),
        done(json!({"id": "scene-tool-mcp", "type": "dynamic_tool", "toolName": "mcp__t3__t3_thread_read", "title": "t3_thread_read",
            "input": {"threadId": "thread:scene", "limit": 20}, "output": [{"type": "text", "text": "{\"title\":\"Timeline polish\",\"status\":\"idle\"}"}]}), 7, 9),
        test,
    ]);
    if let Some(now) = crate::data::iso_ms("2026-12-01T00:00:47.000Z") {
        app.now_ms = now;
    }
}
fn tool_group(app: &mut App) {
    live_turn(app, false);
}
fn tool_group_open(app: &mut App) {
    live_turn(app, false);
    app.ui.chat.timeline.expanded.insert("tools:scene-tool-read-a".into());
}
fn tool_live(app: &mut App) {
    live_turn(app, true);
}
fn thinking(app: &mut App) {
    working(app);
}
fn tool_inspector(app: &mut App) {
    tool_group_open(app);
    app.ui.chat.timeline.expanded.extend(["scene-tool-test".to_owned(), "scene-tool-mcp".to_owned(), "scene-tool-search".to_owned()]);
}
fn server_update_running(app: &mut App) {
    use crate::chat::server_update::{Stage, Status, show};
    showcase(app);
    show(app, Status::Running { stage: Stage::Resuming, from: "0.0.20".into(), target: "0.0.21".into() });
}
fn server_update_failed(app: &mut App) {
    use crate::chat::server_update::{Stage, Status, show};
    showcase(app);
    let message = "Server update failed: The new server did not start.".to_owned();
    show(app, Status::Failed { stage: Stage::Downloading, from: "0.0.20".into(), target: "0.0.21".into(), message });
}
fn link_chips(app: &mut App) {
    showcase(app);
    let other = app.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).find(|t| !t.is_empty()).unwrap_or_default();
    let text = format!(
        "I moved the grouping into [rows.rs](src/chat/timeline/rows.rs:124) and the summary into [`work.rs`](./src/chat/timeline/work.rs). \
         The approach follows [the earlier discussion](t3-thread://v1/{other}); see also the [Rust reference](https://doc.rust-lang.org/reference/).\n\n\
         - Timeline: [mod.rs](src/chat/timeline/mod.rs:40)\n\
         - Composer: [mod.rs](src/chat/composer/mod.rs)"
    );
    push_visible(app, vec![serde_json::json!({"id": "scene-link-chips", "type": "assistant_message", "status": "completed", "text": text,
        "createdAt": "2026-01-01T00:00:00Z"})]);
}
fn user_markdown(app: &mut App) {
    showcase(app);
    let short = "Can you **tighten** the fold?\n- keep `max_lines(7)`\n- add a test";
    let long = "Two things for the timeline:\n\n1. Group tool calls\n2. Show *Thinking* while idle\n\n```rust\nfn derive() {}\n```\n\nThanks!";
    push_visible(app, vec![
        serde_json::json!({"id": "scene-user-md", "type": "user_message", "text": short, "createdAt": "2026-01-01T00:00:00Z", "attachments": []}),
        serde_json::json!({"id": "scene-user-md-long", "type": "user_message", "text": long, "createdAt": "2026-01-01T00:00:01Z", "attachments": []}),
    ]);
}
/// `$skill` tokens in a prompt read as skill chips; an amount and an unknown name stay text.
fn skill_chips(app: &mut App) {
    showcase(app);
    let skill = |name: &str, display: &str, description: &str| {
        serde_json::json!({"name": name, "displayName": display, "description": description, "path": format!("/repo/.claude/skills/{name}/SKILL.md"), "enabled": true})
    };
    let skills = serde_json::json!([skill("review-pr", "Review PR", "Review the open pull request"), skill("release-notes", "", "Draft release notes")]);
    patch_config(app, |c| {
        for provider in c["providers"].as_array_mut().into_iter().flatten() {
            provider["skills"] = skills.clone();
        }
    });
    let text = "$review-pr the retry loop, then $release-notes for v2.\nBudget is $5k; $unknown stays text.";
    push_visible(app, vec![serde_json::json!({"id": "scene-user-skills", "type": "user_message", "text": text, "createdAt": "2026-01-01T00:00:00Z", "attachments": []})]);
}
fn steer(app: &mut App) {
    showcase(app);
    let user = |id: &str, intent: &str, text: &str| {
        serde_json::json!({"id": id, "type": "user_message", "inputIntent": intent, "status": "completed", "text": text, "createdAt": "2026-01-01T00:00:00Z", "attachments": []})
    };
    push_visible(app, vec![
        user("scene-steer-a", "steer", "Actually, skip the docs for now."),
        user("scene-steer-b", "steer", "And keep the old test names."),
        serde_json::json!({"id": "scene-steer-reply", "type": "assistant_message", "status": "completed", "text": "Got it: no docs, same test names.", "createdAt": "2026-01-01T00:00:02Z"}),
        user("scene-steer-c", "promoted_queued_to_steer", "Then run the suite once more."),
    ]);
}

/// A bounded window with older turns on the server.
fn load_earlier(app: &mut App) {
    pending(app);
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.history_cursor = Some("scene-cursor".into());
        v.has_more_history = true;
    }
}
fn load_earlier_error(app: &mut App) {
    load_earlier(app);
    let Route::Thread(id) = app.route.clone() else { return };
    app.ui.chat.timeline.history.errors.insert(id, "The history cursor is no longer valid.".into());
}
fn answered_questions(app: &mut App) {
    pending(app);
    push_visible(app, vec![serde_json::json!({"id": "scene-answered", "type": "user_input_request", "status": "completed", "createdAt": "2026-01-01T00:00:00Z",
        "questionAnswer": {"requestId": "r", "questionTextById": {"q1": "Strict schemas or UI flexibility?", "q2": "Which runtime should the probe target?"},
            "answers": {"q1": ["UI flexibility"], "q2": "Node 24"}}})]);
    app.ui.chat.timeline.expanded.insert("scene-answered".into());
}

/// The showcase's first finished turn with its work deferred by the server, its fold open.
fn deferred_work(app: &mut App) -> Option<String> {
    showcase(app);
    let Route::Thread(thread) = app.route.clone() else { return None };
    let (fold, anchor) = app.state().thread_views.get(&thread).and_then(|v| {
        rows::derive(&v.projection, app.now_ms).into_iter().find_map(|r| match r {
            Row::Work { id, items, .. } => items.first().map(|i| (id, crate::data::s(i, "id").to_owned())),
            _ => None,
        })
    })?;
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&thread)
    {
        for list in ["visibleTurnItems", "turnItems"] {
            for entry in v.projection[list].as_array_mut().into_iter().flatten() {
                let item = if entry["item"].is_object() { &mut entry["item"] } else { entry };
                if item["id"] == anchor.as_str() {
                    item["deferredWork"] = serde_json::json!({"cursor": "scene-work", "itemCount": 240, "lastOrdinal": 100000});
                }
            }
        }
    }
    app.ui.chat.timeline.expanded.insert(fold);
    Some(anchor)
}
fn work_history_loading(app: &mut App) {
    deferred_work(app);
}
fn work_history_error(app: &mut App) {
    if let Some(anchor) = deferred_work(app) {
        app.ui.chat.timeline.history.failed_work.insert(anchor);
    }
}
/// A running command's open row streaming coloured output.
fn command_output(app: &mut App) {
    live_turn(app, true);
    app.ui.chat.timeline.expanded.extend(["tools:scene-tool-read-a".to_owned(), "scene-tool-test".to_owned()]);
    let raw = "\u{1b}[2m$ vp test run src/chat/timeline\u{1b}[22m\n\u{1b}[32m✓\u{1b}[39m rows.rs (14 tests)\n\u{1b}[31m✗\u{1b}[39m work.rs › groups tools\n  expected \u{1b}[33m3\u{1b}[39m, received \u{1b}[33m2\u{1b}[39m\n\u{1b}[42;30m PASS \u{1b}[0m find.rs\nRunning 3 more files… 40%\r\u{1b}[KRunning 3 more files… 80%";
    super::inspector::scene_stream(app, "scene-tool-test", raw, true);
}

/// Another thread's id, for rows that open one.
fn other_thread(app: &App) -> String {
    let Route::Thread(id) = &app.route else { return String::new() };
    app.state().threads.iter().map(|t| crate::data::s(t, "id").to_owned()).find(|t| !t.is_empty() && t != id).unwrap_or_default()
}
/// A failed provider call, a retry in flight and one that recovered, a usage limit with its
/// reset time, and a runtime warning.
fn provider_errors(app: &mut App) {
    use serde_json::json;
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    let at = "2026-03-10T11:58:00Z";
    let overloaded = json!({"class": "transport_error", "message": "Overloaded: the provider is temporarily unable to take requests."});
    push_visible(app, vec![
        json!({"id": "scene-provider-error", "type": "error", "status": "failed", "createdAt": at,
            "failure": {"class": "provider_error", "message": "The provider process exited unexpectedly. Please try again.", "code": null, "retryable": true}}),
        json!({"id": "scene-provider-retrying", "type": "error", "status": "running", "createdAt": at, "failure": overloaded.clone(),
            "retry": {"attempt": 2, "maxAttempts": 5, "retryDelayMs": 1500}}),
        json!({"id": "scene-provider-recovered", "type": "error", "status": "completed", "createdAt": at, "failure": overloaded,
            "retry": {"attempt": 3, "maxAttempts": 5, "retryDelayMs": null}}),
        json!({"id": "scene-usage-limit", "type": "error", "status": "failed", "createdAt": at,
            "failure": {"class": "usage_limit", "message": "You've hit your usage limit.", "resetAt": "2026-03-10T15:30:00Z"}}),
        json!({"id": "scene-system-notice", "type": "system_notice", "status": "completed", "createdAt": at, "message": "Sandbox is off for this turn."}),
    ]);
}
/// Lifecycle rows: a handoff, a created thread, a fork, compactions, an interrupt and a
/// subagent's notice, with every work fold open.
fn lifecycle_rows(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let other = other_thread(app);
    let at = "2026-01-01T00:00:00Z";
    push_visible(app, vec![
        json!({"id": "scene-handoff", "type": "handoff", "status": "completed", "createdAt": at, "fromProviderInstanceIds": ["claudeAgent"],
            "fromModelSelections": [{"instanceId": "claudeAgent", "model": "claude-opus-4-5"}], "toProviderInstanceId": "codex", "toModel": "gpt-5-codex"}),
        json!({"id": "scene-created", "type": "thread_created", "status": "completed", "createdAt": at, "title": "Migrate the billing schema", "targetThreadId": other}),
        json!({"id": "scene-fork", "type": "fork", "status": "completed", "createdAt": at, "source": {"type": "run", "threadId": other, "runId": "scene-run"}}),
        json!({"id": "scene-compacted", "type": "compaction", "status": "completed", "createdAt": at, "beforeTokenCount": 182_000, "afterTokenCount": 24_000}),
        json!({"id": "scene-compact-failed", "type": "compaction", "status": "failed", "createdAt": at, "summary": "The provider rejected the summary request."}),
        json!({"id": "scene-interrupted", "type": "run_interrupt_result", "status": "completed", "createdAt": at, "message": "Stopped by the user."}),
        json!({"id": "scene-subagent-note", "type": "notification", "status": "completed", "outcome": "completed", "createdAt": at,
            "source": {"kind": "subagent", "childThreadId": other}, "summary": "Billing schema migrated"}),
    ]);
    let Route::Thread(id) = app.route.clone() else { return };
    let folds: Vec<String> = app
        .state()
        .thread_views
        .get(&id)
        .map(|v| rows::derive(&v.projection, app.now_ms).iter().filter_map(|r| if let Row::Work { id, .. } = r { Some(id.clone()) } else { None }).collect::<Vec<_>>())
        .unwrap_or_default();
    app.ui.chat.timeline.expanded.extend(folds);
}
/// A subagent's thread, with the divider naming its parent above the timeline.
fn parent_divider(app: &mut App) {
    showcase(app);
    let parent = other_thread(app);
    patch_shell(app, serde_json::json!({"lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}}));
}
/// Prompts from an automation and another agent (open), one T3 Code sent, a failed prompt after
/// a model switch, file cards with a pasted text, and an empty reply.
fn user_attribution(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let other = other_thread(app);
    let at = "2026-01-01T00:00:00Z";
    let user = |id: &str, text: &str, extra: serde_json::Value| {
        let mut item = json!({"id": id, "type": "user_message", "status": "completed", "inputIntent": "turn_start", "text": text, "createdAt": at, "attachments": []});
        for (k, v) in extra.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    push_visible(app, vec![
        user("scene-user-automation", "Check CI on main and summarize any failures.\n\nOpen an issue for each new flaky test.",
            json!({"scheduledTaskId": "task-nightly", "createdBy": "agent", "creationSource": "server"})),
        user("scene-user-agent", "The schema migration is done; please review the diff before I merge.",
            json!({"createdBy": "agent", "creationSource": "mcp", "senderThreadId": other})),
        user("scene-user-t3", "Continue", json!({"createdBy": "agent", "creationSource": "server"})),
        user("scene-user-failed", "Rename the billing tables.", json!({"status": "failed",
            "modelSwitch": {"from": {"instanceId": "codex", "model": "gpt-5-codex"}, "to": {"instanceId": "codex", "model": "gpt-5"}}})),
        user("scene-user-files", "", json!({"attachments": [
            {"type": "file", "id": "scene-pasted", "name": "pasted-text-1.txt", "mimeType": "text/plain", "sizeBytes": 1834, "source": {"_tag": "pasted-text"}},
            {"type": "file", "id": "scene-log", "name": "build.log", "mimeType": "text/plain", "sizeBytes": 48_213}]})),
        json!({"id": "scene-empty-reply", "type": "assistant_message", "status": "interrupted", "streaming": false, "text": "", "createdAt": at}),
    ]);
    app.ui.chat.timeline.expanded.insert("user-fold:scene-user-agent".into());
}

/// Sets the scene clock to `iso`.
fn clock(app: &mut App, iso: &str) {
    if let Some(now) = crate::data::iso_ms(iso) {
        app.now_ms = now;
    }
}
/// A finished turn the user steered once its first work was done: "Worked for" over the whole
/// turn, the steer, then "Continued for" over the work after it.
fn steer_segments(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-03T00:00:{s:02}.000Z");
    push_items(app, "runs", vec![json!({"id": "scene-steered", "ordinal": 996, "status": "completed", "startedAt": at(0), "completedAt": at(42)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| {
        let mut item = json!({"id": id, "type": kind, "runId": "scene-steered", "status": "completed", "createdAt": at(end.saturating_sub(2)),
            "completedAt": at(end), "updatedAt": at(end)});
        for (k, v) in fields.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    push_visible(app, vec![
        item("scene-steered-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Add a retry to the upload client.", "attachments": []})),
        item("scene-steered-think", "reasoning", 3, json!({"text": "**Reading the upload client**"})),
        item("scene-steered-read", "dynamic_tool", 5, json!({"toolName": "Read", "input": {"path": "src/upload/client.rs"}})),
        item("scene-steered-test", "command_execution", 12, json!({"input": "vp test run src/upload", "exitCode": 0})),
        item("scene-steered-steer", "user_message", 14, json!({"inputIntent": "steer", "text": "Use exponential backoff, capped at 30s.", "attachments": [],
            "createdAt": at(14)})),
        item("scene-steered-edit", "file_change", 25, json!({"changes": [{"path": "src/upload/client.rs"}], "fileName": "src/upload/client.rs"})),
        item("scene-steered-retest", "command_execution", 38, json!({"input": "vp test run src/upload", "exitCode": 0})),
        item("scene-steered-reply", "assistant_message", 42, json!({"text": "Added exponential backoff to the upload client, capped at 30s."})),
    ]);
}
fn steer_segments_open(app: &mut App) {
    steer_segments(app);
    app.ui.chat.timeline.expanded.extend(["work:scene-steered".to_owned(), "work:scene-steered:steer:scene-steered-steer".to_owned()]);
}
/// A turn in flight whose first attempt a steer cut off: that attempt's work folds under
/// "Superseded attempt" while the new attempt runs.
fn superseded_attempt(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-02T00:00:{s:02}.000Z");
    let (run, first, second) = ("scene-steer-run", "scene-attempt-1", "scene-attempt-2");
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 997, "status": "running", "rootNodeId": "scene-attempt-1-root", "startedAt": at(0)})]);
    push_items(app, "attempts", vec![
        json!({"id": first, "runId": run, "attemptOrdinal": 1, "rootNodeId": "scene-attempt-1-root", "reason": "initial", "status": "superseded",
            "completedAt": at(14)}),
        json!({"id": second, "runId": run, "attemptOrdinal": 2, "rootNodeId": "scene-attempt-2-root", "reason": "steering_restart", "status": "running"}),
    ]);
    let item = |id: &str, node: &str, kind: &str, status: &str, s: u32, fields: serde_json::Value| {
        let mut item = json!({"id": id, "type": kind, "runId": run, "nodeId": node, "status": status, "createdAt": at(s), "updatedAt": at(s)});
        for (k, v) in fields.as_object().into_iter().flatten() {
            item[k] = v.clone();
        }
        item
    };
    let (old, new) = ("scene-attempt-1-root", "scene-attempt-2-root");
    push_visible(app, vec![
        item("scene-attempt-prompt", old, "user_message", "completed", 0, json!({"inputIntent": "turn_start", "attachments": [],
            "text": "Rename the billing tables and update the migrations."})),
        item("scene-attempt-think", old, "reasoning", "completed", 2, json!({"text": "**Planning the rename**"})),
        item("scene-attempt-search", old, "command_execution", "completed", 4, json!({"input": "rg -l billing_ src/", "exitCode": 0})),
        item("scene-attempt-read", old, "dynamic_tool", "completed", 6, json!({"toolName": "Read", "input": {"path": "src/db/schema.rs"}})),
        item("scene-attempt-reply", old, "assistant_message", "interrupted", 9, json!({"text": "I'll start with the schema, then"})),
        item("scene-attempt-steer", new, "user_message", "completed", 12, json!({"inputIntent": "steer", "attachments": [],
            "text": "Keep the old table names as views."})),
        item("scene-attempt-test", new, "command_execution", "running", 15, json!({"input": "vp test run src/db"})),
    ]);
    clock(app, &at(20));
}
fn superseded_attempt_open(app: &mut App) {
    superseded_attempt(app);
    app.ui.chat.timeline.expanded.insert("attempt-fold:scene-attempt-1".into());
}
/// Four tasks one turn delegated, grouped behind "4 subagents".
fn subagent_group(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-04T00:00:{s:02}.000Z");
    let agent = |n: u32, driver: &str, status: &str, title: &str, done: Option<u32>| {
        json!({"id": format!("scene-agent-{n}"), "type": "subagent", "subagentId": format!("scene-task-{n}"), "runId": "scene-agents-run",
            "providerTurnId": "scene-agents-turn", "driver": driver, "status": status, "title": title, "startedAt": at(n),
            "completedAt": done.map(at), "progress": "Reading the billing migrations", "createdAt": at(n)})
    };
    push_visible(app, vec![
        agent(1, "codex", "running", "Review the schema diff", None),
        agent(2, "claudeAgent", "completed", "Subagent: Update the migrations", Some(30)),
        agent(3, "codex", "failed", "/root/agents/test_runner/", Some(22)),
        agent(4, "opencode", "completed", "Check the docs", Some(18)),
    ]);
    clock(app, &at(48));
}
fn subagent_group_open(app: &mut App) {
    subagent_group(app);
    app.ui.chat.timeline.expanded.insert("subagent-group:scene-agent-1".into());
}
/// A turn in flight compacting its context: the working row says so and nothing thinks.
fn compacting(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-05T00:00:{s:02}.000Z");
    push_items(app, "runs", vec![json!({"id": "scene-compact-run", "ordinal": 995, "status": "running", "startedAt": at(0)})]);
    push_visible(app, vec![
        json!({"id": "scene-compact-prompt", "type": "user_message", "runId": "scene-compact-run", "inputIntent": "turn_start", "status": "completed",
            "text": "Keep going with the migration.", "createdAt": at(0), "attachments": []}),
        json!({"id": "scene-compaction", "type": "compaction", "runId": "scene-compact-run", "status": "running", "createdAt": at(2)}),
    ]);
    clock(app, &at(9));
}

/// The end of the timeline: a subagent's question, a background command, a paused goal, and
/// the thread snoozed.
fn thread_now(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    patch_shell(app, serde_json::json!({"status": "idle", "activityRunStatus": null, "activeRunId": null,
        "pendingBackgroundTasks": [{"taskId": "scene-bg", "kind": "command", "description": "bun run dev --port 5173"}],
        "goal": {"status": "paused", "objective": "Migrate billing to the new schema", "tokensUsed": 48_200, "timeUsedSeconds": 1_260},
        "snoozedUntil": "2026-03-10T14:30:00Z", "snoozedAt": "2026-03-10T11:00:00Z"}));
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-child-question", "title": "Audit the billing schema", "projectId": project,
            "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
            "pendingRuntimeRequest": {"kind": "user_input"}}));
    }
}
/// A Stop the agent has not answered, with Force stop.
fn thread_now_stop(app: &mut App) {
    showcase(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    push_items(app, "runs", vec![serde_json::json!({"id": "scene-stop-run", "ordinal": 9_999, "status": "running"})]);
    push_items(app, "turnItems", vec![serde_json::json!({"id": "scene-stop", "type": "run_interrupt_request", "runId": "scene-stop-run", "ordinal": 99_999,
        "startedAt": "2026-03-10T11:59:18Z", "updatedAt": "2026-03-10T11:59:48Z", "unresponsiveAt": "2026-03-10T11:59:48Z"})]);
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(config) = state.config.as_mut()
    {
        config["environment"]["capabilities"]["threadForceStop"] = true.into();
    }
}
/// A side chat open beside the showcase thread it forked from (SideChatPane).
fn side_pane(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let Some(shell) = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).cloned() else { return };
        // Forked from the parent as it was last updated, and hidden from the sidebar like any side chat.
        state.threads.push(serde_json::json!({"id": "scene-side", "title": "Why does the retry back off?", "projectId": shell["projectId"], "status": "idle",
            "createdAt": shell["updatedAt"], "updatedAt": shell["updatedAt"], "modelSelection": shell["modelSelection"],
            "runtimeMode": shell["runtimeMode"], "interactionMode": shell["interactionMode"],
            "lineage": {"parentThreadId": parent, "rootThreadId": parent, "relationshipToParent": "fork", "sideChat": true}}));
        if let Some(view) = state.thread_views.get(&parent) {
            let projection = view.projection.clone();
            state.thread_views.insert("scene-side".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
        }
    }
    app.ui.chat.side.open.insert(parent, "scene-side".into());
}
/// The side chat's composer with a typed follow-up and a file queued, the caret in it.
fn side_pane_composer(app: &mut App) {
    side_pane(app);
    let d = app.ui.chat.composer.drafts.entry("scene-side".into()).or_default();
    d.edit = rune::TextEdit::new("Would jitter make the retries spread out more?");
    d.edit.set_caret(d.edit.text.len());
    d.edit.focused = true;
    let file = std::env::temp_dir().join("s5-scene-attachments").join("retry-trace.log");
    if file.parent().is_some_and(|dir| std::fs::create_dir_all(dir).is_ok()) {
        let _ = std::fs::write(&file, vec![b'.'; 12 * 1024]);
    }
    d.attachments.push(file.display().to_string());
}
/// A running side chat: its composer offers Stop, while the chat's own composer stays idle.
fn side_pane_stop(app: &mut App) {
    side_pane(app);
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(side) = state.threads.iter_mut().find(|r| crate::data::s(r, "id") == "scene-side")
    {
        side["activeRunId"] = "scene-side-run".into();
        side["status"] = "running".into();
    }
}
/// A closed side chat opened again from its parent (openSideChatTab), the caret in its composer.
fn side_reopen(app: &mut App) {
    side_pane(app);
    app.ui.chat.side.open.clear();
    crate::chat::side::open_tab(app, "scene-side");
}
/// The details card's "Side chats": eight of the showcase's side chats in each state, one
/// running, the first six shown with "Show 2 more".
fn side_details_list(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        let chats = [
            ("Why does the retry back off?", "running", "running"),
            ("Naming ideas for the cache", "idle", "completed"),
            ("Is the lockfile drift real?", "idle", "failed"),
            ("Quick check on the CI matrix", "idle", "interrupted"),
            ("Summarise the review thread", "idle", "waiting"),
            ("Compare the two parsers", "idle", ""),
            ("What changed in the schema?", "idle", "completed"),
            ("Draft the release note", "idle", "completed"),
        ];
        for (n, (title, status, activity)) in chats.into_iter().enumerate() {
            let activity = if activity.is_empty() { serde_json::Value::Null } else { activity.into() };
            state.threads.push(serde_json::json!({"id": format!("scene-side-{n}"), "title": title, "projectId": project, "status": status,
                "activityRunStatus": activity, "createdAt": format!("2026-03-10T11:{:02}:00Z", 50 - n),
                "lineage": {"parentThreadId": parent, "rootThreadId": parent, "relationshipToParent": "fork", "sideChat": true}}));
        }
    }
}
/// A side chat waiting on its own question: the drawer docks on the side chat's composer, while
/// the chat's composer stays clear.
fn side_composer_question(app: &mut App) {
    side_pane(app);
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let asked = state.thread_views.values().find(|v| crate::data::s(&v.projection["thread"], "title") == "Timeline question fixture").map(|v| v.projection.clone());
        if let (Some(asked), Some(side)) = (asked, state.thread_views.get_mut("scene-side")) {
            for key in ["runtimeRequests", "turnItems"] {
                let extra: Vec<serde_json::Value> = asked[key].as_array().into_iter().flatten().filter(|v| key == "runtimeRequests" || v["type"] == "user_input_request").cloned().collect();
                match side.projection[key].as_array_mut() {
                    Some(list) => list.extend(extra),
                    None => side.projection[key] = extra.into(),
                }
            }
        }
    }
}
/// A side chat in plan mode with a proposed plan: "Plan ready" and Implement on its composer.
fn side_composer_plan(app: &mut App) {
    side_pane(app);
    app.client_settings.values.insert("planModeEnabled".into(), true.into());
    if let crate::data::Source::Fixture(state) = &mut app.src {
        if let Some(side) = state.threads.iter_mut().find(|r| crate::data::s(r, "id") == "scene-side") {
            side["interactionMode"] = "plan".into();
        }
        if let Some(view) = state.thread_views.get_mut("scene-side") {
            view.projection["plans"] = serde_json::json!([{"id": "scene-side-plan", "kind": "proposed_plan", "status": "active", "runId": null,
                "markdown": "# Add jitter to the retry backoff\n\n- Spread retries with full jitter\n- Cap the delay at 30 seconds"}]);
        }
    }
}
/// A subagent's header: its parent in the breadcrumb, and the linked environment it came from.
fn header_subagent(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-subagent", "title": "Audit the billing schema", "projectId": project, "status": "idle",
            "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}, "linkOrigin": {"sessionId": "scene-link", "label": "Studio"}}));
        if let Some(view) = state.thread_views.get(&parent) {
            let projection = view.projection.clone();
            state.thread_views.insert("scene-subagent".into(), s5_net::ThreadView { projection, synchronized: true, sequence: 0, ..Default::default() });
        }
    }
    app.navigate(Route::Thread("scene-subagent".into()));
}
/// The new-thread draft over its project.
fn draft_hero(app: &mut App) {
    app.navigate(Route::Home { project_id: None });
    crate::scene_clock(app);
}
/// The draft with no project to build in.
fn draft_hero_empty(app: &mut App) {
    if let crate::data::Source::Fixture(state) = &mut app.src {
        state.projects.clear();
    }
    draft_hero(app);
}

fn patch_config(app: &mut App, f: impl FnOnce(&mut serde_json::Value)) {
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(config) = state.config.as_mut()
    {
        f(config);
    }
}
fn feedback_sent(app: &mut App) {
    use crate::chat::composer::banners::FeedbackStatus;
    let Route::Thread(id) = app.route.clone() else { return };
    app.ui.chat.banners.feedback.insert(id, vec![("scene-feedback-sent".into(), FeedbackStatus::Sent("019a4c1e-7f2b-7c41-9b3e-5d2a8f6c1e90".into()))]);
}
fn managed_ceiling(app: &mut App) {
    patch_shell(app, serde_json::json!({"runtimeMode": "full-access"}));
    patch_config(app, |c| c["managedPolicy"] = serde_json::json!({"organization": "Example Corp", "lockedSettings": [], "limits": {"maxRuntimeMode": "auto-accept-edits"}}));
}
/// Codex feedback sent, and one that could not be.
fn composer_banner_feedback(app: &mut App) {
    use crate::chat::composer::banners::FeedbackStatus;
    showcase(app);
    feedback_sent(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let failed = ("scene-feedback-failed".into(), FeedbackStatus::Failed("The Codex session has no logs to upload yet.".into()));
    app.ui.chat.banners.feedback.entry(id).or_default().push(failed);
}
/// The new-thread draft on the fallback model while the default is out of usage.
fn composer_banner_fallback(app: &mut App) {
    draft_hero(app);
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    let default = serde_json::json!({"instanceId": "claudeAgent", "model": "claude-opus-5-5"});
    if let crate::data::Source::Fixture(state) = &mut app.src {
        for project in &mut state.projects {
            project["defaultModelSelection"] = default.clone();
        }
    }
    patch_config(app, |c| {
        c["settings"]["defaultModelSelection"] = default.clone();
        c["settings"]["fallbackModelSelection"] = serde_json::json!({"instanceId": "opencode", "model": "opencode/big-pickle"});
        for p in c["providers"].as_array_mut().into_iter().flatten() {
            match crate::data::s(p, "instanceId").to_owned().as_str() {
                "claudeAgent" => p["usageLimits"] = serde_json::json!({"checkedAt": "2026-03-10T11:59:00Z",
                    "windows": [{"id": "five_hour", "label": "5-hour", "usedPercent": 100, "resetsAt": "2026-03-10T15:30:00Z"}]}),
                "opencode" => p["usageLimits"] = serde_json::json!({"checkedAt": "2026-03-10T11:59:00Z", "windows": []}),
                _ => {}
            }
        }
    });
}
/// A thread whose permissions sit above the organization's ceiling.
fn composer_banner_runtime_ceiling(app: &mut App) {
    showcase(app);
    managed_ceiling(app);
}
/// Several notices at once: the failed server update first, then feedback, the usage-limit
/// recovery and the permissions ceiling.
fn composer_banner_stack(app: &mut App) {
    usage_limit_banner(app);
    feedback_sent(app);
    managed_ceiling(app);
    use crate::chat::server_update::{Stage, Status, show};
    let message = "Server update failed: The new server did not start.".to_owned();
    show(app, Status::Failed { stage: Stage::Downloading, from: "0.0.20".into(), target: "0.0.21".into(), message });
}
/// The end of the timeline: two children waiting (each name opens it), two subagents with the
/// newest progress shown, and the goal.
fn timeline_now_banners(app: &mut App) {
    showcase(app);
    let Route::Thread(parent) = app.route.clone() else { return };
    app.now_ms = crate::data::iso_ms("2026-03-10T12:00:00Z").unwrap_or(app.now_ms);
    patch_shell(app, serde_json::json!({"status": "idle", "activityRunStatus": null, "activeRunId": null,
        "pendingBackgroundTasks": [
            {"taskId": "scene-sub-audit", "kind": "subagent", "description": "Audit the billing schema"},
            {"taskId": "scene-sub-migrate", "kind": "subagent", "description": "Draft the migration"}],
        "goal": {"status": "active", "objective": "Migrate billing to the new schema", "tokensUsed": 48_200}}));
    push_items(app, "turnItems", vec![
        serde_json::json!({"id": "scene-sub-audit", "type": "subagent", "progress": "Reading migrations/0042_billing.sql", "startedAt": "2026-03-10T11:50:00Z", "updatedAt": "2026-03-10T11:58:00Z"}),
        serde_json::json!({"id": "scene-sub-migrate", "type": "subagent", "progress": "Writing the backfill  for invoices", "startedAt": "2026-03-10T11:52:00Z", "updatedAt": "2026-03-10T11:59:30Z"}),
    ]);
    if let crate::data::Source::Fixture(state) = &mut app.src {
        let project = state.threads.iter().find(|r| crate::data::s(r, "id") == parent).map(|r| r["projectId"].clone()).unwrap_or_default();
        state.threads.push(serde_json::json!({"id": "scene-child-approval", "title": "Check the invoice totals", "projectId": project,
            "status": "running", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
            "pendingRuntimeRequest": {"kind": "command"}}));
        state.threads.push(serde_json::json!({"id": "scene-side-chat", "title": "Naming ideas", "projectId": project,
            "status": "idle", "lineage": {"parentThreadId": parent, "sideChat": true}, "pendingRuntimeRequest": {"kind": "user_input"}}));
    }
}

/// A stopped turn's earlier reply shows no meta; its last reply's meta sits below its work.
fn stopped_replies(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-10T00:00:{s:02}.000Z");
    let run = "scene-stopped-replies";
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 994, "status": "interrupted", "startedAt": at(0), "completedAt": at(30)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| scene_item(run, &at, id, kind, end, fields);
    push_visible(app, vec![
        item("scene-replies-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Tidy the billing module.", "attachments": []})),
        item("scene-replies-first", "assistant_message", 4, json!({"text": "Starting with the expiry helpers."})),
        item("scene-replies-read", "dynamic_tool", 6, json!({"toolName": "Read", "input": {"path": "src/billing/expiry.rs"}})),
        item("scene-replies-last", "assistant_message", 12, json!({"text": "The helpers are tidy. Running the tests next."})),
        item("scene-replies-test", "command_execution", 28, json!({"input": "vp test run src/billing", "status": "interrupted"})),
        item("scene-replies-result", "run_interrupt_result", 30, json!({"message": "Stopped by the user."})),
    ]);
}

/// A finished turn's open fold holding an `html_render` page, its picture seeded.
fn html_render(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-11T00:00:{s:02}.000Z");
    let run = "scene-html-run";
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 995, "status": "completed", "startedAt": at(0), "completedAt": at(20)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| scene_item(run, &at, id, kind, end, fields);
    let page = json!({"htmlRender": {"attachmentId": "scene-html-page", "title": "Q3 revenue by region", "height": 420}});
    push_visible(app, vec![
        item("scene-html-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "Chart Q3 revenue by region.", "attachments": []})),
        item("scene-html-tool", "dynamic_tool", 14, json!({"toolName": "mcp__t3-code__html_render", "input": {"title": "Q3 revenue by region"},
            "output": {"structuredContent": page}})),
        item("scene-html-reply", "assistant_message", 20, json!({"text": "The chart is above. EMEA led the quarter."})),
    ]);
    app.ui.chat.timeline.expanded.insert(format!("work:{run}"));
    crate::panel::html_frame::scene_seed(app, "scene-html-page", 420.);
}

/// An MCP app's tool call, its app running in the row with the border it asks for.
fn mcp_app(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let at = |s: u32| format!("2026-12-13T00:00:{s:02}.000Z");
    let run = "scene-mcp-run";
    push_items(app, "runs", vec![json!({"id": run, "ordinal": 996, "status": "completed", "startedAt": at(0), "completedAt": at(20)})]);
    let item = |id: &str, kind: &str, end: u32, fields: serde_json::Value| scene_item(run, &at, id, kind, end, fields);
    let reference = json!({"t3McpApp": {"attachmentId": "scene-mcp-page", "server": "weather", "tool": "forecast", "resourceUri": "ui://weather/forecast.html", "prefersBorder": true}});
    let tool = item("scene-mcp-tool", "dynamic_tool", 14, json!({"toolName": "weather.forecast", "input": {"city": "Oslo"}, "output": {"structuredContent": reference}}));
    push_visible(app, vec![
        item("scene-mcp-prompt", "user_message", 0, json!({"inputIntent": "turn_start", "text": "What's the week like in Oslo?", "attachments": []})),
        tool.clone(),
        item("scene-mcp-reply", "assistant_message", 20, json!({"text": "Mostly clear, warming through Friday."})),
    ]);
    app.ui.chat.timeline.expanded.insert(format!("work:{run}"));
    crate::panel::mcp_app::scene_seed(app, &tool, 360.);
}

/// `timeline-html-render` with its page shot by real Chrome (`S5_CHROME`).
fn html_render_chrome(app: &mut App) {
    html_render(app);
    crate::panel::html_frame::scene_shoot(app, "scene-html-page");
}

/// `timeline-mcp-app` with its app run live in real Chrome through the host protocol.
fn mcp_app_chrome(app: &mut App) {
    mcp_app(app);
    crate::panel::mcp_app::scene_live(app, "scene-mcp-tool");
}

/// A delegated task running in a linked environment, named after its row title.
fn subagent_remote(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let child = other_thread(app);
    let at = |s: u32| format!("2026-12-12T00:00:{s:02}.000Z");
    push_visible(app, vec![json!({"id": "scene-remote-task", "type": "subagent", "subagentId": "scene-remote-agent", "status": "running",
        "title": "Run the GPU benchmarks", "driver": "codex", "childThreadId": child, "progress": "Warming up the cluster",
        "startedAt": at(0), "createdAt": at(0), "updatedAt": at(5),
        "remoteChild": {"environmentId": "scene-env-build", "threadId": "scene-remote-thread", "label": "Build box"}})]);
    if let Some(now) = crate::data::iso_ms(&at(42)) {
        app.now_ms = now;
    }
}

/// An Antigravity subagent's thread: the bar in the composer's place, with "Subagent work".
fn subagent_bar(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let parent = other_thread(app);
    let instance = "scene-antigravity";
    patch_config(app, |config| {
        let provider = json!({"instanceId": instance, "driver": "antigravity", "displayName": "Antigravity", "status": "ready", "models": [
            {"slug": "gemini-3-pro", "name": "Gemini 3 Pro", "capabilities": {"optionDescriptors": [
                {"id": "reasoningEffort", "type": "select", "options": [{"id": "high", "label": "High"}, {"id": "low", "label": "Low"}]}]}}]});
        match config["providers"].as_array_mut() {
            Some(providers) => providers.push(provider),
            None => config["providers"] = json!([provider]),
        }
    });
    patch_shell(app, json!({"creationSource": "provider", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"},
        "modelSelection": {"instanceId": instance, "model": "gemini-3-pro", "options": [{"id": "reasoningEffort", "value": "high"}]}}));
    let started = "2026-12-13T00:00:00.000Z";
    push_items(app, "nodes", vec![json!({"id": "scene-subagent-root", "kind": "root_turn", "runId": null, "status": "running", "startedAt": started})]);
    if let Some(start) = crate::data::iso_ms(started) {
        app.now_ms = start + 42_000;
    }
}

/// The subagent bar for an Antigravity account with an accent: the instance badge on its mark,
/// and the reasoning level the provider reports for a selection that states none.
fn subagent_bar_instance(app: &mut App) {
    use serde_json::json;
    subagent_bar(app);
    patch_config(app, |config| {
        let providers = config["providers"].as_array_mut();
        for p in providers.into_iter().flatten().filter(|p| p["instanceId"] == "scene-antigravity") {
            p["displayName"] = json!("Studio Gemini");
            p["accentColor"] = json!("#2563eb");
        }
    });
    let selection = json!({"instanceId": "scene-antigravity", "model": "gemini-3-pro"});
    patch_shell(app, json!({"modelSelection": selection.clone()}));
    let Route::Thread(id) = app.route.clone() else { return };
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.projection["thread"]["activeProviderThreadId"] = json!("scene-provider-thread");
        v.projection["thread"]["modelSelection"] = selection.clone();
        let mut reported = selection;
        reported["options"] = json!([{"id": "reasoningEffort", "value": "low"}]);
        let provider = json!({"id": "scene-provider-thread", "providerInstanceId": "scene-antigravity", "nativeMetadata": {"modelSelection": reported}});
        match v.projection["providerThreads"].as_array_mut() {
            Some(list) => list.push(provider),
            None => v.projection["providerThreads"] = json!([provider]),
        }
    }
}

/// The `/usage-limits` panel with two reset credits banked on the Claude login.
fn usage_reset_credits(app: &mut App) {
    if let Some((_, open)) = crate::chat::composer::SCENES.iter().find(|(name, _)| *name == "composer-usage-limits") {
        open(app);
    }
    patch_config(app, |config| {
        for p in config["providers"].as_array_mut().into_iter().flatten().filter(|p| p["instanceId"] == "claudeAgent") {
            p["usageLimits"]["resetCredits"] = serde_json::json!({"availableCount": 2, "nextExpiresAt": "2026-12-08T00:00:00.000Z", "nextCreditId": "credit-1"});
        }
    });
}

/// A workflow coordinator's thread: its script opens with the workflow, its agents in phases
/// from the parent's coordinator row, and the script folded (WorkflowOpeningBlock).
fn workflow_opening(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let parent = other_thread(app);
    let Route::Thread(id) = app.route.clone() else { return };
    patch_shell(app, json!({"creationSource": "provider", "lineage": {"parentThreadId": parent, "relationshipToParent": "subagent"}}));
    let base = crate::data::iso_ms("2026-12-01T00:00:00.000Z").unwrap_or(0);
    app.now_ms = base + 95_000;
    let script = "// Audit billing for rounding bugs, one phase at a time.\n\
        // Survey maps the call sites, Fix patches them, Verify adds tests.\n\
        export const meta = { name: \"billing-audit\", phases: [\"Survey\", \"Fix\", \"Verify\"] };\n\
        \n\
        export default async function run({ phase, agent }) {\n\
        \x20 const sites = await phase(\"Survey\", () => agent(\"Map rounding call sites\"));\n\
        \x20 await phase(\"Fix\", () => Promise.all(sites.map((site) => agent(`Patch ${site}`))));\n\
        \x20 await phase(\"Verify\", () => agent(\"Write regression tests\"));\n\
        \x20 return sites.length;\n\
        }";
    let agents = json!([
        {"index": 0, "label": "Map rounding call sites", "state": "completed", "phaseIndex": 0, "startedAt": base, "completedAt": base + 21_000,
            "prompt": "Find every place billing rounds money", "result": "Found 14 call sites across 5 files", "childThreadId": "scene-wf-map"},
        {"index": 1, "label": "Check currency conversions", "state": "completed", "phaseIndex": 0, "startedAt": base + 1_000, "completedAt": base + 34_000,
            "prompt": "Check how conversions round", "result": "Conversions round twice in `fx.ts`", "childThreadId": "scene-wf-fx"},
        {"index": 2, "label": "Patch invoice totals", "state": "running", "phaseIndex": 1, "startedAt": base + 40_000,
            "prompt": "Round once, at the invoice boundary", "childThreadId": "scene-wf-invoice"},
        {"index": 3, "label": "Patch refunds", "state": "failed", "phaseIndex": 1, "startedAt": base + 40_000, "completedAt": base + 52_000,
            "prompt": "Round refunds like invoices", "result": "Refund fixtures are missing the currency field"},
        {"index": 4, "label": "Write regression tests", "state": "queued", "phaseIndex": 2, "phaseTitle": "Verify", "prompt": "Cover half-cent amounts in every currency"}]);
    let coordinator = json!({"id": "scene-workflow", "threadId": parent, "childThreadId": id, "origin": "provider_native", "driver": "claudeAgent",
        "providerInstanceId": "claudeAgent", "title": "Audit billing for rounding bugs", "prompt": script, "status": "running",
        "startedAt": "2026-12-01T00:00:00.000Z", "completedAt": null, "updatedAt": "2026-12-01T00:01:35.000Z", "result": null,
        "workflow": {"name": "billing-audit", "phases": [{"index": 0, "title": "Survey"}, {"index": 1, "title": "Fix"}], "agents": agents}});
    let crate::data::Source::Fixture(state) = &mut app.src else { return };
    if let Some(v) = state.thread_views.get_mut(&id) {
        let mut message = String::new();
        if let Some(first) = v.projection["messages"].as_array_mut().and_then(|m| m.iter_mut().find(|m| m["role"] == "user")) {
            first["text"] = json!(script);
            first["workflowScript"] = json!(true);
            message = crate::data::s(first, "id").to_owned();
        }
        for list in ["visibleTurnItems", "turnItems"] {
            let entries = v.projection[list].as_array_mut().into_iter().flatten();
            if let Some(item) = entries.map(|e| if e["item"].is_object() { &mut e["item"] } else { e }).find(|i| i["type"] == "user_message") {
                item["text"] = json!(script);
                item["messageId"] = json!(message);
                item["workflowScript"] = json!(true);
            }
        }
    }
    state.thread_views.entry(parent).or_default().projection["subagents"] = json!([coordinator]);
}

/// Six prompts in the timeline's minimap, the third under the pointer with its turn's preview.
fn timeline_minimap(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let turns = [
        ("Why does the upload retry twice?", "The retry loop re-sends after a **timeout** even when the first request landed, as <t3-thread://v2/env-1/thread-1> found."),
        ("Make it idempotent instead", "Each upload now carries an idempotency key, so a second send returns the first result."),
        ("Add a test for the double send", "Added `upload.retry.test.ts`, which sends twice and expects one stored file."),
        ("Does the key survive a restart?", "Yes. It's derived from the file's hash and the session, so a restart keeps it."),
        ("Ship it", "Committed as `fix(upload): retries no longer store twice`."),
    ];
    let mut items = Vec::new();
    for (n, (prompt, reply)) in turns.iter().enumerate() {
        items.push(json!({"id": format!("scene-minimap-prompt-{n}"), "type": "user_message", "text": prompt, "createdAt": "2026-01-01T00:00:00Z", "attachments": []}));
        items.push(json!({"id": format!("scene-minimap-reply-{n}"), "type": "assistant_message", "text": reply, "streaming": false, "createdAt": "2026-01-01T00:00:01Z"}));
    }
    push_visible(app, items);
    app.ui.chat.timeline.minimap.active = Some(2);
}

/// A message just sent, shown before the server echoes it and anchored near the list's top.
fn optimistic_send(app: &mut App) {
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    let message = super::pending::message("scene-optimistic", "Can you also cover the empty state with a test?", &[], serde_json::json!([]), false);
    super::pending::show(app, &id, message);
}

/// A file drag over the chat: "Drop files to attach" covers it.
fn drop_overlay(app: &mut App) {
    showcase(app);
    app.ui.chat.drop_over = true;
}

/// The machine dropped a few seconds ago and a retry is under way.
fn offline_reconnecting(app: &mut App) {
    showcase(app);
    super::banners::scene_outage(app, s5_net::Conn::Connecting, 5_000);
}

/// A saved machine offline long enough to offer "Disconnect server".
fn offline_disconnect(app: &mut App) {
    showcase(app);
    app.ui.env.saved.push(crate::env::Saved { id: "scene-studio".into(), label: "Studio".into(), base: "https://studio.example.ts.net".into(), bearer: String::new() });
    app.ui.env.active = "scene-studio".into();
    super::banners::scene_outage(app, s5_net::Conn::Closed("Connection refused (os error 61)".into()), 30_000);
}

/// A guest link in use: the header's status, a sent guest message under the guest's name, and
/// two waiting for Send or Dismiss.
fn guest_share(app: &mut App) {
    use serde_json::json;
    showcase(app);
    let Route::Thread(id) = app.route.clone() else { return };
    push_visible(app, vec![json!({
        "id": "scene-guest-sent", "type": "user_message", "messageId": "scene-guest-sent", "createdBy": "user",
        "text": "Ada (guest) wrote: Could the empty state link to the docs?", "createdAt": "2026-01-01T00:00:02Z",
        "context": {"version": 1, "records": [], "guest": {"name": "Ada", "text": "Could the empty state link to the docs?"}},
    })]);
    if let crate::data::Source::Fixture(state) = &mut app.src
        && let Some(v) = state.thread_views.get_mut(&id)
    {
        v.projection["thread"]["guestShare"] = json!({
            "id": "scene-share", "host": "scene.t3.codes", "createdAt": "2026-01-01T00:00:00Z", "expiresAt": "2099-01-01T00:00:00Z",
            "guestName": "Ada",
            "messages": [
                {"id": "scene-guest-1", "text": "Also, the button label reads \"Sumbit\".", "status": "pending", "submittedAt": "2026-01-01T00:00:03Z"},
                {"id": "scene-guest-2", "text": "Can we ship this today?", "status": "pending", "submittedAt": "2026-01-01T00:00:04Z"},
                {"id": "scene-guest-0", "text": "Hi!", "status": "sent", "submittedAt": "2026-01-01T00:00:01Z"},
            ],
        });
    }
}
