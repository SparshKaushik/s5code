//! An open tool row's details (V2ItemInspector, CommandOutputPanel, ShellCommandBlock): the
//! command and its output, a tool's arguments and result, a change's files, a search's hits.
//! The wire leaves command and tool output off (WireProjection.ts), so the first open fetches
//! the item with `orchestration.getTurnItem`. An open running command streams its output with
//! `orchestration.subscribeCommandOutput` instead, on servers that offer it.
use super::{copy_button, inline_button, rows};
use crate::App;
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use rune::{El, ScrollSpec, col, row, text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct State {
    /// `getTurnItem` requests: the row's item id and the revision asked for.
    pending: HashMap<u64, (String, String)>,
    /// Fetched items by row item id, with their revision, or why the fetch failed.
    loaded: HashMap<String, (String, Result<Value, String>)>,
    /// Streamed output of open command rows, by row item id.
    streams: HashMap<String, Stream>,
}

const OUTPUT_METHOD: &str = "orchestration.subscribeCommandOutput";

/// `TERMINAL_OUTPUT_TAIL_CHARS`: the streamed tail a row keeps.
const STREAM_TAIL: usize = 64 * 1024;

/// One open command row's output stream (CommandOutputPanel). It stays after the command ends,
/// showing the final output, until the row closes.
#[derive(Default)]
struct Stream {
    /// The thread on screen when it opened.
    route: String,
    payload: Value,
    watching: bool,
    raw: String,
    truncated: bool,
    /// A frame has arrived.
    started: bool,
    /// The server sends no more: `running: false`, or the stream exited.
    ended: bool,
    error: Option<String>,
    revision: u64,
}

/// Folds one `OrchestrationV2CommandOutputFrame`, or the stream's exit, into `stream`.
fn fold_frame(stream: &mut Stream, event: &Value) {
    stream.revision += 1;
    if let Some(exit) = event.get("error") {
        stream.ended = true;
        if exit["_tag"] != "Success" {
            let message = s(super::link::failure(&exit["cause"]), "message");
            stream.error = Some(if message.is_empty() { "the output stream failed".to_owned() } else { message.to_owned() });
        }
        return;
    }
    let text = s(event, "text");
    match s(event, "kind") {
        "replace" => stream.raw = text.to_owned(),
        "append" => stream.raw.push_str(text),
        _ => return,
    }
    stream.started = true;
    stream.error = None;
    stream.truncated = event["truncated"] == true;
    stream.ended |= event["running"] == false;
    if stream.raw.len() > 2 * STREAM_TAIL {
        let mut start = stream.raw.len() - STREAM_TAIL;
        while !stream.raw.is_char_boundary(start) {
            start += 1;
        }
        let start = stream.raw[start..].find('\n').map_or(start, |i| start + i + 1);
        stream.raw.drain(..start);
        stream.truncated = true;
    }
}

/// Every row and its item in a projection, windowed or not.
fn items_of(p: &Value) -> Vec<(&Value, &Value)> {
    match p["visibleTurnItems"].as_array() {
        Some(list) => list.iter().map(|r| (r, &r["item"])).collect(),
        None => rows::array(p, "turnItems").iter().map(|i| (i, i)).collect(),
    }
}

/// Streams the output of open running commands in the thread on screen, stops once a command
/// ends, and forgets a stream once its row closes.
fn sync_streams(app: &mut App) -> bool {
    let ui = &app.ui.chat.timeline;
    if ui.inspector.streams.is_empty() && ui.expanded.is_empty() {
        return false;
    }
    let route = match app.frame_route() {
        crate::Route::Thread(id) => id.clone(),
        _ => String::new(),
    };
    let streaming = app.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["commandOutputStreaming"] == true);
    let mut running = HashSet::new();
    let mut want = Vec::new();
    if streaming && let Some(view) = app.state().thread_views.get(&route) {
        for (entry, item) in items_of(&view.projection) {
            let id = s(item, "id");
            if s(item, "type") != "command_execution" || !rows::item_active(item) || !ui.expanded.contains(id) {
                continue;
            }
            running.insert(id.to_owned());
            if !ui.inspector.streams.contains_key(id) {
                let thread = Some(s(entry, "sourceThreadId")).filter(|v| !v.is_empty()).unwrap_or(route.as_str());
                let target = Some(s(entry, "sourceItemId")).filter(|v| !v.is_empty()).unwrap_or(id);
                want.push((id.to_owned(), json!({"threadId": thread, "itemId": target})));
            }
        }
    }
    let closed: Vec<String> = ui.inspector.streams.iter().filter(|(id, st)| st.route != route || !ui.expanded.contains(*id)).map(|(id, _)| id.clone()).collect();
    let done: Vec<String> = ui.inspector.streams.iter().filter(|(id, st)| st.watching && !running.contains(*id)).map(|(id, _)| id.clone()).collect();
    let mut net = app.src.net();
    let streams = &mut app.ui.chat.timeline.inspector.streams;
    let mut changed = !closed.is_empty();
    for id in done.iter().chain(&closed) {
        if let Some(st) = streams.get_mut(id)
            && st.watching
        {
            st.watching = false;
            if let Some(net) = net.as_deref_mut() {
                net.unwatch_resource(OUTPUT_METHOD, st.payload.clone());
            }
        }
    }
    for id in &closed {
        streams.remove(id);
    }
    let Some(net) = net else { return changed };
    for (id, payload) in want {
        net.watch_resource(OUTPUT_METHOD, payload.clone());
        streams.insert(id, Stream { route: route.clone(), payload, watching: true, ..Stream::default() });
        changed = true;
    }
    for st in streams.values_mut().filter(|st| st.watching) {
        for event in net.take_resource_events(OUTPUT_METHOD, st.payload.clone()) {
            fold_frame(st, &event);
            changed = true;
        }
        if st.ended {
            st.watching = false;
            net.unwatch_resource(OUTPUT_METHOD, st.payload.clone());
        }
    }
    changed
}

/// Shows `raw` as an open row's streamed output, as a scene's stand-in for a live stream.
pub(super) fn scene_stream(app: &mut App, id: &str, raw: &str, running: bool) {
    let crate::Route::Thread(route) = app.frame_route() else { return };
    let mut stream = Stream { route: route.clone(), ..Stream::default() };
    fold_frame(&mut stream, &json!({"kind": "replace", "text": raw, "truncated": true, "running": running}));
    app.ui.chat.timeline.inspector.streams.insert(id.to_owned(), stream);
}

/// What to fetch for one row, captured by its press handler.
pub struct Ask {
    thread: String,
    target: String,
    id: String,
    revision: String,
}

/// `turnItemDetailRevision`: a running item keeps one revision, so it fetches once while it
/// runs and again once it ends.
fn revision(item: &Value) -> String {
    if matches!(s(item, "status"), "idle" | "pending" | "running" | "waiting") { "live".into() } else { s(item, "updatedAt").into() }
}

fn summarized(value: &Value) -> bool {
    value["truncated"] == true && value["summary"].is_string()
}

/// `turnItemNeedsDetailFetch`.
fn needs_fetch(item: &Value) -> bool {
    match s(item, "type") {
        "command_execution" => item["outputOmitted"] == true,
        "dynamic_tool" => item["outputOmitted"] == true || summarized(&item["input"]),
        _ => false,
    }
}

pub fn ask(app: &App, item: &Value) -> Option<Ask> {
    if !needs_fetch(item) {
        return None;
    }
    let crate::Route::Thread(route) = app.frame_route() else { return None };
    let thread = Some(s(item, "sourceThreadId")).filter(|v| !v.is_empty()).unwrap_or(route.as_str()).to_owned();
    let target = Some(s(item, "sourceItemId")).filter(|v| !v.is_empty()).unwrap_or(s(item, "id")).to_owned();
    Some(Ask { thread, target, id: s(item, "id").to_owned(), revision: revision(item) })
}

/// Asks for the item unless it is loaded at this revision or already on its way.
pub fn fetch(app: &mut App, ask: &Ask) {
    let state = &app.ui.chat.timeline.inspector;
    let fresh = state.loaded.get(&ask.id).is_some_and(|(r, result)| *r == ask.revision && result.is_ok());
    if fresh || state.pending.values().any(|(id, _)| *id == ask.id) {
        return;
    }
    let Some(net) = app.src.net() else { return };
    let rpc = net.request("orchestration.getTurnItem", json!({"threadId": ask.thread, "itemId": ask.target}));
    app.ui.chat.timeline.inspector.pending.insert(rpc, (ask.id.clone(), ask.revision.clone()));
}

pub fn poll(app: &mut App) -> bool {
    let streamed = sync_streams(app);
    streamed | poll_fetches(app)
}

fn poll_fetches(app: &mut App) -> bool {
    if app.ui.chat.timeline.inspector.pending.is_empty() {
        return false;
    }
    let Some(net) = app.src.net() else { return false };
    let state = &mut app.ui.chat.timeline.inspector;
    let mut changed = false;
    state.pending.retain(|rpc, (id, revision)| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        let result = match reply {
            Ok(v) if v["item"].is_object() => Ok(v["item"].clone()),
            Ok(_) => Err("the item is no longer available".to_owned()),
            Err(e) => Err(Some(s(super::link::failure(&e), "message")).filter(|m| !m.is_empty()).map_or_else(|| e.to_string(), str::to_owned)),
        };
        state.loaded.insert(std::mem::take(id), (std::mem::take(revision), result));
        false
    });
    changed
}

#[derive(Debug, PartialEq)]
enum Section {
    /// A labelled monospace block.
    Code(&'static str, String),
    Note(String),
    Error(String),
    /// An answered question (QuestionAnswerHistory): the question, then its answer indented.
    Answer(String, String),
}

/// `getQuestionAnswerText`: strings, lists joined by commas, and `{answers}` records.
fn answer_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(answer_text).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(", "),
        Value::Object(record) => record.get("answers").map(answer_text).unwrap_or_default(),
        _ => String::new(),
    }
}

/// Every question the answer names, in the order web lists them, with its text and answer.
fn question_answers(answer: &Value) -> Vec<(String, String)> {
    let mut ids: Vec<&String> = Vec::new();
    for key in ["questionTextById", "answers", "attachmentsByQuestionId"] {
        for id in answer[key].as_object().into_iter().flat_map(|m| m.keys()) {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    }
    ids.into_iter()
        .map(|id| {
            let names: Vec<&str> = rows::array(&answer["attachmentsByQuestionId"], id).iter().map(|a| s(a, "name")).filter(|n| !n.is_empty()).collect();
            let text = [answer_text(&answer["answers"][id.as_str()]), names.join(", ")].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n");
            (s(&answer["questionTextById"], id).to_owned(), text)
        })
        .filter(|(q, a)| !q.is_empty() || !a.is_empty())
        .collect()
}

/// Output past this many bytes keeps its tail, where failures usually are.
const MAX_OUTPUT: usize = 32_000;

fn cap(text: &str) -> String {
    if text.len() <= MAX_OUTPUT {
        return text.to_owned();
    }
    let mut start = text.len() - MAX_OUTPUT;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    let start = text[start..].find('\n').map_or(start, |i| start + i + 1);
    format!("… earlier output not shown\n{}", &text[start..])
}

/// `commandOutputText`: a `{"stdout","stderr","interrupted"}` record as its two streams.
fn command_output_text(output: &str) -> String {
    if !output.trim_start().starts_with("{\"stdout\"") {
        return output.to_owned();
    }
    let Ok(v) = serde_json::from_str::<Value>(output.trim()) else { return output.to_owned() };
    let (Some(out), Some(err), true) = (v["stdout"].as_str(), v["stderr"].as_str(), v["interrupted"].is_boolean()) else {
        return output.to_owned();
    };
    [out, err].into_iter().filter(|p| !p.trim().is_empty()).collect::<Vec<_>>().join("\n")
}

/// `prettyJsonText`: a JSON document, or one per line, pretty-printed; anything else as is.
fn pretty_json_text(text: &str) -> String {
    let trimmed = text.trim();
    if !trimmed.starts_with(['{', '[']) {
        return text.to_owned();
    }
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        return serde_json::to_string_pretty(&v).unwrap_or_else(|_| text.to_owned());
    }
    let docs: Option<Vec<Value>> = trimmed.lines().filter(|l| !l.trim().is_empty()).map(|l| serde_json::from_str(l.trim()).ok()).collect();
    match docs {
        Some(docs) => docs.iter().filter_map(|d| serde_json::to_string_pretty(d).ok()).collect::<Vec<_>>().join("\n\n"),
        None => text.to_owned(),
    }
}

/// `formatJsonValue`: nothing for null, blank text, `{}` and `[]`.
fn format_json_value(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) if text.trim().is_empty() => None,
        _ => serde_json::to_string_pretty(value).ok().filter(|json| json != "{}" && json != "[]"),
    }
}

/// `textFromBlocks`: MCP content blocks as their text, or `None` when they are not all text.
fn text_from_blocks(value: &Value, depth: usize) -> Option<String> {
    if depth > 4 {
        return None;
    }
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(list) => {
            let parts: Option<Vec<String>> = list.iter().map(|b| text_from_blocks(b, depth + 1)).collect();
            parts.map(|p| p.into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join("\n"))
        }
        Value::Object(map) => {
            match s(value, "type") {
                "text" if value["text"].is_string() => return Some(s(value, "text").to_owned()),
                "image" => return Some("[image]".into()),
                "resource_link" if value["uri"].is_string() => return Some(s(value, "uri").to_owned()),
                "resource" => {
                    if let Some(text) = value["resource"]["text"].as_str().or_else(|| value["resource"]["uri"].as_str()) {
                        return Some(text.to_owned());
                    }
                }
                _ => {}
            }
            let keys: Vec<&String> = map.keys().filter(|k| *k != "isError" && *k != "is_error").collect();
            (keys.len() == 1 && keys[0] == "content").then(|| text_from_blocks(&value["content"], depth + 1)).flatten()
        }
        _ => None,
    }
}

/// `formatToolValue`.
fn format_tool_value(value: &Value) -> Option<String> {
    if value.is_null() {
        return None;
    }
    match text_from_blocks(value, 0) {
        Some(text) => (!text.trim().is_empty()).then(|| pretty_json_text(&text)),
        None => format_json_value(value),
    }
}

/// `toolCallLines`: an object's arguments one per line, anything else formatted whole.
fn tool_call_lines(args: &Value) -> Option<String> {
    match args {
        Value::Object(_) if summarized(args) => Some(s(args, "summary").to_owned()),
        Value::Object(map) => {
            let lines: Vec<String> = map
                .iter()
                .map(|(key, v)| match v {
                    Value::String(text) if !text.is_empty() => format!("{key}: {text}"),
                    other => format!("{key}: {other}"),
                })
                .collect();
            (!lines.is_empty()).then(|| lines.join("\n"))
        }
        _ => format_tool_value(args),
    }
}

/// The inspector's sections for `item`, reading output from `fetched` once it has arrived.
fn sections(item: &Value, fetched: Option<&Value>, root: Option<&str>) -> Vec<Section> {
    let full = fetched.unwrap_or(item);
    let path = |p: &str| rows::workspace_path(p, root);
    let mut out = Vec::new();
    match s(item, "type") {
        "command_execution" => {
            let command = s(item, "input").trim();
            if !command.is_empty() {
                out.push(Section::Code("Command", command.to_owned()));
            }
            let output = s(full, "output");
            if !output.trim().is_empty() {
                out.push(Section::Code("Output", cap(&command_output_text(output))));
            } else if full["outputOmitted"] != true && !super::work::active(item) {
                out.push(Section::Note("No output.".into()));
            }
            if let Some(code) = item["exitCode"].as_i64().filter(|c| *c != 0) {
                out.push(Section::Error(format!("exit {code}")));
            }
        }
        "dynamic_tool" => {
            if let Some(args) = tool_call_lines(&full["input"]) {
                out.push(Section::Code("Input", cap(&args)));
            }
            if full["outputOmitted"] != true
                && let Some(output) = format_tool_value(&full["output"])
            {
                out.push(Section::Code("Output", cap(&output)));
            }
        }
        "file_change" => {
            let changes = rows::array(item, "changes");
            for change in changes {
                let shown = path(s(change, "path"));
                out.push(Section::Note(match (s(change, "operation"), s(change, "oldPath")) {
                    (_, old) if !old.is_empty() && old != s(change, "path") => format!("{} → {shown}", path(old)),
                    ("", _) => shown,
                    (operation, _) => format!("{operation} {shown}"),
                }));
            }
            if changes.is_empty() && !s(item, "fileName").is_empty() {
                out.push(Section::Note(path(s(item, "fileName"))));
            }
            if let (Some(add), Some(del)) = (item["additions"].as_u64(), item["deletions"].as_u64()) {
                out.push(Section::Note(format!("+{add} −{del}")));
            }
            if s(item, "status") == "failed"
                && let Some(diff) = item["diffStr"].as_str().filter(|d| !d.trim().is_empty())
            {
                out.push(Section::Code("Diff", cap(diff)));
            }
        }
        "file_search" => {
            if let Some(pattern) = item["pattern"].as_str().filter(|p| !p.trim().is_empty()) {
                out.push(Section::Code("Pattern", pattern.to_owned()));
            }
            let hits: Vec<String> = rows::array(item, "results")
                .iter()
                .map(|r| {
                    let at = match r["line"].as_u64() {
                        Some(n) => format!("{}:{n}", path(s(r, "fileName"))),
                        None => path(s(r, "fileName")),
                    };
                    match r["preview"].as_str().map(str::trim).filter(|p| !p.is_empty()) {
                        Some(preview) => format!("{at}\n  {preview}"),
                        None => at,
                    }
                })
                .collect();
            if !hits.is_empty() {
                out.push(Section::Code("Results", cap(&hits.join("\n"))));
            }
        }
        "web_search" => {
            let patterns: Vec<&str> = rows::array(item, "patterns").iter().filter_map(Value::as_str).collect();
            if !patterns.is_empty() {
                out.push(Section::Code("Queries", patterns.join("\n")));
            }
            let hits: Vec<String> = rows::array(item, "results")
                .iter()
                .map(|r| {
                    let (title, url) = (s(r, "title").trim(), s(r, "url"));
                    let head = if title.is_empty() { [url, ""] } else { [title, url] };
                    head.into_iter().chain([s(r, "snippet").trim()]).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("\n")
                })
                .collect();
            if !hits.is_empty() {
                out.push(Section::Code("Results", cap(&hits.join("\n\n"))));
            }
        }
        "user_input_request" => {
            out.extend(question_answers(&item["questionAnswer"]).into_iter().map(|(q, a)| Section::Answer(q, a)));
        }
        _ => {}
    }
    out
}

fn code(app: &App, key: String, label: &'static str, body: String, t: &T) -> El {
    let head = row()
        .w_full()
        .center_y()
        .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0))
        .child(copy_button(app, &body, t, "Copy"));
    let block = col()
        .key(key)
        .w_full()
        .max_h(320.0)
        .radius(6.0)
        .clip_rounded(6.0)
        .fill(t.c.foreground.alpha_mul(0.04))
        .scroll(ScrollSpec { stick_bottom: false })
        .child(
            col()
                .w_full()
                .px(10.0)
                .py(8.0)
                .child(text(body).font(theme::MONO_XS.lh(18.0)).color(t.c.foreground.alpha_mul(0.85)).selectable(true).wrap_anywhere()),
        );
    col().w_full().gap(4.0).child(head).child(block)
}

/// The open row's body, or `None` for kinds without one (the caller shows its plain detail).
pub fn view(app: &App, item: &Value, t: &T) -> Option<El> {
    let state = &app.ui.chat.timeline.inspector;
    let id = s(item, "id");
    let loaded = state.loaded.get(id);
    let fetched = loaded.and_then(|(_, r)| r.as_ref().ok());
    let root = super::workspace_root(app);
    let stream = state.streams.get(id);
    let omitted = needs_fetch(item) && stream.is_none();
    let pending = state.pending.values().any(|(i, _)| i == id);
    let copied = app.ui.chat.timeline.copied.as_ref().is_some_and(|(c, _)| super::copied(app, c));
    let colors = format!("{:?}", (t.c.foreground, t.c.muted_foreground, t.c.destructive, t.dark));
    let streamed = stream.map(|st| (st.revision, st.watching));
    let version = (s(item, "updatedAt"), s(item, "status"), loaded.map(|(r, result)| (r.clone(), result.is_ok())), pending, copied, colors, root.clone(), streamed);
    let mut sections = sections(item, fetched, root.as_deref());
    if stream.is_some() {
        sections.retain(|x| !matches!(x, Section::Code("Output", _) | Section::Note(_)));
    }
    if sections.is_empty() && !omitted && stream.is_none() {
        return None;
    }
    // Output can run to tens of kilobytes, and the timeline rebuilds every second.
    Some(rune::memo(format!("inspector-{id}"), version, || {
        let mut out = col().w_full().gap(8.0).pl(28.0).pr(4.0).pb(4.0);
        let mut stream = stream;
        for (index, section) in sections.into_iter().enumerate() {
            let command = matches!(section, Section::Code("Command", _));
            out = out.child(match section {
                Section::Code(label, body) => code(app, format!("inspect-{id}-{index}"), label, body, t),
                Section::Note(line) => text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).selectable(true).wrap_anywhere(),
                Section::Error(line) => text(line).font(theme::TEXT_XS).color(t.c.destructive),
                Section::Answer(question, answer) => col()
                    .w_full()
                    .gap(4.0)
                    .children((!question.is_empty()).then(|| text(question).font(theme::TEXT_SM).color(t.c.muted_foreground).selectable(true).wrap()))
                    .children((!answer.is_empty()).then(|| text(answer).font(theme::TEXT_SM).color(t.c.muted_foreground).selectable(true).wrap().pl(12.0))),
            });
            if command && let Some(st) = stream.take() {
                out = out.child(stream_view(app, id, st, t));
            }
        }
        if let Some(st) = stream {
            out = out.child(stream_view(app, id, st, t));
        }
        if omitted {
            let stale = loaded.is_some_and(|(r, _)| *r != revision(item));
            let note = match loaded {
                _ if pending => Some("Loading output…".to_owned()),
                Some((_, Err(e))) => Some(format!("Couldn't load output: {e}")),
                _ => None,
            };
            out = out.children(note.map(|n| text(n).font(theme::TEXT_XS).color(t.c.muted_foreground)));
            if !pending && (fetched.is_none() || stale)
                && let Some(ask) = ask(app, item)
            {
                let label = if loaded.is_some() { "Reload output" } else { "Load output" };
                out = out.child(inline_button(label, false, t).on_press(move |app: &mut App| fetch(app, &ask)));
            }
        }
        out
    }))
}

/// Streamed output: ANSI colours on the theme's terminal palette, following the bottom while
/// the command runs (CommandOutputPanel).
fn stream_view(app: &App, id: &str, stream: &Stream, t: &T) -> El {
    let note = |line: &str| text(line).font(theme::TEXT_XS).color(t.c.muted_foreground);
    if stream.raw.is_empty() {
        return match (&stream.error, stream.started) {
            (Some(e), _) => text(format!("Couldn't load output: {e}")).font(theme::TEXT_XS).color(t.c.destructive),
            (None, false) => note("Loading output…"),
            (None, true) if !stream.ended => col(),
            (None, true) => note("No output."),
        };
    }
    let (shown, runs) = ansi::render(&stream.raw);
    let palette = if t.dark { &ansi::DARK } else { &ansi::LIGHT };
    let base = t.c.foreground.alpha_mul(0.85);
    let spans: Vec<rune::Span> = runs.into_iter().map(|(range, pen)| ansi::span(range, pen, palette, base)).collect();
    let head = row()
        .w_full()
        .center_y()
        .child(text("Output").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0))
        .child(copy_button(app, &shown, t, "Copy"));
    let block = col()
        .key(format!("inspect-{id}-stream"))
        .w_full()
        .max_h(320.0)
        .radius(6.0)
        .clip_rounded(6.0)
        .fill(t.c.foreground.alpha_mul(0.04))
        .scroll(ScrollSpec { stick_bottom: true })
        .child(
            col()
                .w_full()
                .px(10.0)
                .py(8.0)
                .child(text(shown).font(theme::MONO_XS.lh(18.0)).color(base).spans(spans).selectable(true).wrap_anywhere()),
        );
    col()
        .w_full()
        .gap(4.0)
        .children(stream.truncated.then(|| note("Earlier output not shown")))
        .child(head)
        .child(block)
        .children(stream.error.as_ref().map(|e| text(format!("Couldn't load output: {e}")).font(theme::TEXT_XS).color(t.c.destructive)))
}

/// SGR colours and the line edits (carriage returns, `CSI K`, cursor moves) a terminal applies,
/// enough to show command output as a terminal would (`terminalOutputSpans`). Other escapes drop.
mod ansi {
    use rune::{Color, Span};
    use std::ops::Range;

    pub type Rgb = [u8; 3];

    /// `TERMINAL_PALETTES`.
    pub const DARK: [Rgb; 16] = [
        [0x3b, 0x40, 0x48], [0xf4, 0x70, 0x67], [0x57, 0xab, 0x5a], [0xc6, 0x90, 0x26], [0x53, 0x9b, 0xf5], [0xb0, 0x83, 0xf0], [0x39, 0xc5, 0xcf], [0xad, 0xba, 0xc7],
        [0x76, 0x83, 0x90], [0xff, 0x93, 0x8a], [0x6b, 0xc4, 0x6d], [0xda, 0xaa, 0x3f], [0x6c, 0xb6, 0xff], [0xdc, 0xbd, 0xfb], [0x56, 0xd4, 0xdd], [0xcd, 0xd9, 0xe5],
    ];
    pub const LIGHT: [Rgb; 16] = [
        [0x24, 0x29, 0x2f], [0xcf, 0x22, 0x2e], [0x11, 0x63, 0x29], [0x7d, 0x4e, 0x00], [0x05, 0x50, 0xae], [0x82, 0x50, 0xdf], [0x1b, 0x7c, 0x83], [0x6e, 0x77, 0x81],
        [0x57, 0x60, 0x6a], [0xa4, 0x0e, 0x26], [0x1a, 0x7f, 0x37], [0x63, 0x3c, 0x01], [0x09, 0x69, 0xda], [0x66, 0x39, 0xba], [0x31, 0x92, 0xaa], [0x8c, 0x95, 0x9f],
    ];

    /// Cursor moves past this column are clamped, so a stray escape can't pad megabytes.
    const MAX_COL: usize = 1000;
    /// Styled runs past this many render plain, as `terminalOutputSpans` caps its spans.
    const MAX_RUNS: usize = 2000;

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub enum Ink {
        #[default]
        Default,
        Index(u8),
        Rgb(Rgb),
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq)]
    pub struct Pen {
        pub fg: Ink,
        pub bg: Ink,
        pub bold: bool,
        pub dim: bool,
        pub italic: bool,
        pub underline: bool,
        pub inverse: bool,
    }

    fn sgr(pen: &mut Pen, params: &str) {
        let codes: Vec<u16> = params.split(';').map(|p| p.parse().unwrap_or(0)).collect();
        let mut i = 0;
        while i < codes.len() {
            match codes[i] {
                0 => *pen = Pen::default(),
                1 => pen.bold = true,
                2 => pen.dim = true,
                3 => pen.italic = true,
                4 => pen.underline = true,
                7 => pen.inverse = true,
                22 => {
                    pen.bold = false;
                    pen.dim = false;
                }
                23 => pen.italic = false,
                24 => pen.underline = false,
                27 => pen.inverse = false,
                c @ 30..=37 => pen.fg = Ink::Index((c - 30) as u8),
                39 => pen.fg = Ink::Default,
                c @ 40..=47 => pen.bg = Ink::Index((c - 40) as u8),
                49 => pen.bg = Ink::Default,
                c @ 90..=97 => pen.fg = Ink::Index((c - 82) as u8),
                c @ 100..=107 => pen.bg = Ink::Index((c - 92) as u8),
                c @ (38 | 48) => {
                    let byte = |at: usize| codes.get(at).map(|&v| v.min(255) as u8);
                    let ink = match codes.get(i + 1).copied() {
                        Some(5) => {
                            i += 2;
                            byte(i).map(Ink::Index)
                        }
                        Some(2) => {
                            i += 4;
                            match (byte(i - 2), byte(i - 1), byte(i)) {
                                (Some(r), Some(g), Some(b)) => Some(Ink::Rgb([r, g, b])),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(ink) = ink {
                        if c == 38 { pen.fg = ink } else { pen.bg = ink }
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn put(line: &mut Vec<(char, Pen)>, col: &mut usize, c: char, pen: Pen) {
        while line.len() < *col {
            line.push((' ', Pen::default()));
        }
        if *col < line.len() {
            line[*col] = (c, pen);
        } else {
            line.push((c, pen));
        }
        *col += 1;
    }

    /// The text `raw` leaves on a terminal, and its styled runs as byte ranges.
    pub fn render(raw: &str) -> (String, Vec<(Range<usize>, Pen)>) {
        let mut lines: Vec<Vec<(char, Pen)>> = vec![Vec::new()];
        let (mut row, mut col) = (0usize, 0usize);
        let mut pen = Pen::default();
        let mut chars = raw.chars();
        while let Some(c) = chars.next() {
            match c {
                '\n' => {
                    row += 1;
                    col = 0;
                }
                '\r' => col = 0,
                '\u{8}' => col = col.saturating_sub(1),
                '\t' => {
                    let to = (col / 8 + 1) * 8;
                    while col < to.min(MAX_COL) {
                        put(&mut lines[row], &mut col, ' ', pen);
                    }
                }
                '\u{1b}' => match chars.next() {
                    Some('[') => {
                        let mut params = String::new();
                        let mut last = None;
                        for c in chars.by_ref() {
                            if ('\u{40}'..='\u{7e}').contains(&c) {
                                last = Some(c);
                                break;
                            }
                            params.push(c);
                        }
                        let n = params.trim_start_matches('?').split(';').next().and_then(|p| p.parse::<usize>().ok()).unwrap_or(0);
                        let step = n.clamp(1, MAX_COL);
                        match last {
                            Some('m') => sgr(&mut pen, &params),
                            Some('K') => match n {
                                1 => lines[row].iter_mut().take(col + 1).for_each(|cell| *cell = (' ', Pen::default())),
                                2 => lines[row].clear(),
                                _ => lines[row].truncate(col),
                            },
                            Some('J') if n >= 2 => {
                                lines = vec![Vec::new()];
                                row = 0;
                                col = 0;
                            }
                            Some('J') => {
                                lines[row].truncate(col);
                                lines.truncate(row + 1);
                            }
                            Some('A') => row = row.saturating_sub(step),
                            Some('B') => row = (row + step).min(lines.len() - 1),
                            Some('C') => col = (col + step).min(MAX_COL),
                            Some('D') => col = col.saturating_sub(step),
                            Some('G') => col = step - 1,
                            _ => {}
                        }
                    }
                    Some(']') => {
                        while let Some(c) = chars.next() {
                            if c == '\u{7}' {
                                break;
                            }
                            if c == '\u{1b}' {
                                chars.next();
                                break;
                            }
                        }
                    }
                    _ => {}
                },
                c if c.is_control() => {}
                c => {
                    if col < MAX_COL {
                        put(&mut lines[row], &mut col, c, pen);
                    }
                }
            }
            if row >= lines.len() {
                lines.resize_with(row + 1, Vec::new);
            }
        }
        let mut text = String::new();
        let mut runs: Vec<(Range<usize>, Pen)> = Vec::new();
        for (index, line) in lines.iter().enumerate() {
            if index > 0 {
                text.push('\n');
            }
            for &(c, p) in line {
                let start = text.len();
                text.push(c);
                if p == Pen::default() {
                    continue;
                }
                match runs.last_mut() {
                    Some((range, last)) if *last == p && range.end == start => range.end = text.len(),
                    _ if runs.len() < MAX_RUNS => runs.push((start..text.len(), p)),
                    _ => {}
                }
            }
        }
        if text.ends_with('\n') {
            text.pop();
        }
        (text, runs)
    }

    fn ink(ink: Ink, palette: &[Rgb; 16]) -> Option<Rgb> {
        match ink {
            Ink::Default => None,
            Ink::Index(n) if n < 16 => Some(palette[n as usize]),
            Ink::Index(n) if n >= 232 => {
                let level = 8 + (n - 232) * 10;
                Some([level; 3])
            }
            Ink::Index(n) => {
                let cube = n - 16;
                let channel = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
                Some([channel(cube / 36), channel(cube / 6 % 6), channel(cube % 6)])
            }
            Ink::Rgb(c) => Some(c),
        }
    }

    fn luminance(c: Rgb) -> f32 {
        let channel = |v: u8| {
            let s = v as f32 / 255.0;
            if s <= 0.03928 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(c[0]) + 0.7152 * channel(c[1]) + 0.0722 * channel(c[2])
    }

    fn contrast(a: Rgb, b: Rgb) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    fn color(c: Rgb) -> Color {
        let channel = |v: u8| rune::color::srgb_decode(v as f32 / 255.0);
        Color { r: channel(c[0]), g: channel(c[1]), b: channel(c[2]), a: 1.0 }
    }

    /// One run's span (`spanStyle`): a badge's text stays readable on its background.
    pub fn span(range: Range<usize>, pen: Pen, palette: &[Rgb; 16], base: Color) -> Span {
        let (mut fg, mut bg) = (ink(pen.fg, palette), ink(pen.bg, palette));
        if pen.inverse {
            (fg, bg) = (bg, fg.or(Some(palette[7])));
        }
        if let Some(back) = bg
            && fg.is_none_or(|front| contrast(front, back) < 3.0)
        {
            fg = Some(if luminance(back) > 0.35 { [0x1f, 0x23, 0x28] } else { [0xff; 3] });
        }
        let ink = fg.map_or(base, color);
        let ink = if pen.dim { ink.alpha(0.65) } else { ink };
        let mut span = Span::new(range.start, range.end);
        if fg.is_some() || pen.dim {
            span = span.color(ink);
        }
        if let Some(back) = bg {
            span = span.bg(color(back), 0.0);
        }
        if pen.bold {
            span = span.bold();
        }
        if pen.italic {
            span = span.italic();
        }
        if pen.underline {
            span = span.underline(ink);
        }
        span
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answered_questions_pair_each_question_with_its_answer() {
        let answer = json!({
            "requestId": "r",
            "questionTextById": {"q1": "Strict schemas or UI flexibility?"},
            "answers": {"q1": ["UI flexibility"], "q2": {"answers": ["a", "b"]}},
            "attachmentsByQuestionId": {"q2": [{"type": "file", "id": "f", "name": "trace.log", "mimeType": "text/plain", "sizeBytes": 3}]}
        });
        assert_eq!(
            question_answers(&answer),
            vec![("Strict schemas or UI flexibility?".to_owned(), "UI flexibility".to_owned()), (String::new(), "a, b\ntrace.log".to_owned())]
        );
    }

    #[test]
    fn commands_show_their_output_streams_and_failure() {
        let item = json!({"type":"command_execution","status":"failed","input":" vp test ","exitCode":2,"outputOmitted":true});
        assert_eq!(sections(&item, None, None), [Section::Code("Command", "vp test".into()), Section::Error("exit 2".into())]);
        let fetched = json!({"output":"{\"stdout\":\"ok\\n\",\"stderr\":\"boom\",\"interrupted\":false}"});
        assert_eq!(
            sections(&item, Some(&fetched), None),
            [Section::Code("Command", "vp test".into()), Section::Code("Output", "ok\n\nboom".into()), Section::Error("exit 2".into())]
        );
        let quiet = json!({"type":"command_execution","status":"completed","input":"true"});
        assert_eq!(sections(&quiet, None, None)[1], Section::Note("No output.".into()));
    }

    #[test]
    fn tools_show_arguments_by_line_and_text_output() {
        let item = json!({"type":"dynamic_tool","input":{"path":"src/a.rs","limit":20},"output":[{"type":"text","text":"{\"a\":1}"}]});
        assert_eq!(
            sections(&item, None, None),
            [Section::Code("Input", "limit: 20\npath: src/a.rs".into()), Section::Code("Output", "{\n  \"a\": 1\n}".into())]
        );
        assert_eq!(format_tool_value(&json!({})), None);
        assert_eq!(format_tool_value(&json!({"content":[{"type":"text","text":"hi"},{"type":"image"}]})), Some("hi\n[image]".into()));
        assert_eq!(tool_call_lines(&json!({"truncated":true,"summary":"{path: …}"})), Some("{path: …}".into()));
    }

    #[test]
    fn changes_and_searches_list_their_files() {
        let change = json!({"type":"file_change","additions":3,"deletions":1,"changes":[{"path":"/w/demo/a.rs","operation":"update"},{"path":"/w/demo/b.rs","oldPath":"/w/demo/c.rs"}]});
        assert_eq!(
            sections(&change, None, Some("/w/demo")),
            [Section::Note("update demo/a.rs".into()), Section::Note("demo/c.rs → demo/b.rs".into()), Section::Note("+3 −1".into())]
        );
        let search = json!({"type":"file_search","pattern":"fn main","results":[{"fileName":"src/main.rs","line":3,"preview":" fn main() {"}]});
        assert_eq!(sections(&search, None, None)[1], Section::Code("Results", "src/main.rs:3\n  fn main() {".into()));
    }

    #[test]
    fn long_output_keeps_its_tail_from_a_line_start() {
        let text = format!("{}\nlast line", "x".repeat(MAX_OUTPUT));
        let capped = cap(&text);
        assert!(capped.starts_with("… earlier output not shown\n"));
        assert!(capped.ends_with("\nlast line") || capped.ends_with("last line"));
        assert!(capped.len() < text.len());
    }

    #[test]
    fn streamed_frames_replace_append_and_end() {
        let mut stream = Stream { watching: true, ..Stream::default() };
        fold_frame(&mut stream, &json!({"kind":"replace","text":"a\n","truncated":false,"running":true}));
        fold_frame(&mut stream, &json!({"kind":"append","text":"b","truncated":true,"running":true}));
        assert_eq!((stream.raw.as_str(), stream.truncated, stream.ended, stream.started), ("a\nb", true, false, true));
        fold_frame(&mut stream, &json!({"kind":"replace","text":"done","truncated":false,"running":false}));
        assert_eq!((stream.raw.as_str(), stream.truncated, stream.ended), ("done", false, true));
        assert_eq!(stream.error, None);

        let mut failed = Stream::default();
        fold_frame(&mut failed, &json!({"error":{"_tag":"Failure","cause":[{"_tag":"Fail","error":{"message":"Command output is unavailable."}}]}}));
        assert!(failed.ended);
        assert_eq!(failed.error.as_deref(), Some("Command output is unavailable."));
        let mut exited = Stream::default();
        fold_frame(&mut exited, &json!({"error":{"_tag":"Success","value":null}}));
        assert!(exited.ended && exited.error.is_none());

        let mut long = Stream::default();
        fold_frame(&mut long, &json!({"kind":"replace","text":"x\n".repeat(STREAM_TAIL + 1),"truncated":false,"running":true}));
        assert!(long.truncated && long.raw.len() <= STREAM_TAIL && long.raw.starts_with('x'));
    }

    #[test]
    fn terminal_output_keeps_colours_and_applies_line_edits() {
        use ansi::{Ink, Pen, render};
        let red = Pen { fg: Ink::Index(1), ..Pen::default() };
        assert_eq!(render("\u{1b}[31mred\u{1b}[0m plain\n"), ("red plain".to_owned(), vec![(0..3, red)]));
        assert_eq!(render("10%\r50%\r100%\ndone").0, "100%\ndone");
        assert_eq!(render("loading…\r\u{1b}[Kok").0, "ok");
        assert_eq!(render("\u{1b}]0;title\u{7}a\tb\u{1b}[?25l").0, "a       b");
        assert_eq!(render("1\n2\n\u{1b}[1Ax").0, "1\nx");
        let (text, runs) = render("\u{1b}[1;38;5;196mA\u{1b}[38;2;1;2;3mB\u{1b}[22;39mC");
        assert_eq!(text, "ABC");
        assert_eq!(runs, vec![
            (0..1, Pen { fg: Ink::Index(196), bold: true, ..Pen::default() }),
            (1..2, Pen { fg: Ink::Rgb([1, 2, 3]), bold: true, ..Pen::default() }),
        ]);
    }

    #[test]
    fn running_items_share_one_revision() {
        assert_eq!(revision(&json!({"status":"running","updatedAt":"a"})), "live");
        assert_eq!(revision(&json!({"status":"completed","updatedAt":"b"})), "b");
        assert!(needs_fetch(&json!({"type":"dynamic_tool","input":{"truncated":true,"summary":"…"}})));
        assert!(!needs_fetch(&json!({"type":"command_execution","output":"x"})));
    }
}
