//! An open tool row's details (V2ItemInspector, CommandOutputPanel, ShellCommandBlock): the
//! command and its output, a tool's arguments and result, a change's files, a search's hits.
//! The wire leaves command and tool output off (WireProjection.ts), so the first open fetches
//! the item with `orchestration.getTurnItem`.
use super::{copy_button, inline_button, rows};
use crate::App;
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use rune::{El, ScrollSpec, col, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct State {
    /// `getTurnItem` requests: the row's item id and the revision asked for.
    pending: HashMap<u64, (String, String)>,
    /// Fetched items by row item id, with their revision, or why the fetch failed.
    loaded: HashMap<String, (String, Result<Value, String>)>,
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
            Err(e) => Err(Some(s(&e, "message")).filter(|m| !m.is_empty()).map_or_else(|| e.to_string(), str::to_owned)),
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
    let omitted = needs_fetch(item);
    let pending = state.pending.values().any(|(i, _)| i == id);
    let copied = app.ui.chat.timeline.copied.as_ref().is_some_and(|(c, _)| super::copied(app, c));
    let colors = format!("{:?}", (t.c.foreground, t.c.muted_foreground, t.c.destructive));
    let version = (s(item, "updatedAt"), s(item, "status"), loaded.map(|(r, result)| (r.clone(), result.is_ok())), pending, copied, colors, root.clone());
    let sections = sections(item, fetched, root.as_deref());
    if sections.is_empty() && !omitted {
        return None;
    }
    // Output can run to tens of kilobytes, and the timeline rebuilds every second.
    Some(rune::memo(format!("inspector-{id}"), version, || {
        let mut out = col().w_full().gap(8.0).pl(28.0).pr(4.0).pb(4.0);
        for (index, section) in sections.into_iter().enumerate() {
            out = out.child(match section {
                Section::Code(label, body) => code(app, format!("inspect-{id}-{index}"), label, body, t),
                Section::Note(line) => text(line).font(theme::TEXT_XS).color(t.c.muted_foreground).selectable(true).wrap_anywhere(),
                Section::Error(line) => text(line).font(theme::TEXT_XS).color(t.c.destructive),
            });
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn running_items_share_one_revision() {
        assert_eq!(revision(&json!({"status":"running","updatedAt":"a"})), "live");
        assert_eq!(revision(&json!({"status":"completed","updatedAt":"b"})), "b");
        assert!(needs_fetch(&json!({"type":"dynamic_tool","input":{"truncated":true,"summary":"…"}})));
        assert!(!needs_fetch(&json!({"type":"command_execution","output":"x"})));
    }
}
