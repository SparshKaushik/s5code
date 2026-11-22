//! HtmlRenderFrame's "Open full screen" (HtmlRenderDialog). Rune draws no HTML in-process, so the
//! page opens in the browser panel, whose own title, zoom and close stand in for the dialog's.
//! The inline frame is the page's picture (`panel::html_frame`).

use rune::El;
use serde_json::{Value, json};

use crate::theme::T;
use crate::{App, data};

#[derive(Default)]
pub struct State {
    /// The `assets.createUrl` in flight for the page being opened.
    pending: Option<u64>,
}

/// htmlRenderFileName: the title made safe for a file name, at most 120 characters.
pub fn file_name(title: &str) -> String {
    let cleaned: String = title.chars().map(|c| if c.is_control() || "\\/:*?\"<>|".contains(c) { ' ' } else { c }).collect();
    let name: String = cleaned.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(120).collect();
    let name = name.trim();
    format!("{}.html", if name.is_empty() { "Page" } else { name })
}

/// Opens an `html_render` attachment full size, as the frame's expand button does.
pub fn open(s: &mut App, attachment_id: &str, title: &str) {
    let resource = json!({"resource": {"_tag": "attachment", "attachmentId": attachment_id, "fileName": file_name(title), "mimeType": "text/html", "disposition": "inline"}});
    match s.src.net() {
        Some(net) => s.ui.chat.details.html_render.pending = Some(net.request("assets.createUrl", resource)),
        None => unavailable(s),
    }
}

fn unavailable(s: &mut App) {
    crate::shell::palette::toast_error_brief(s, "Page unavailable", Some("Reconnect to the environment and try again."));
}

const T3_SERVERS: [&str; 6] = ["t3-code", "t3_code", "t3code", "t3 code", "t3-code-direct", "t3_code_direct"];

/// resolveT3McpToolId(name) == "html_render": bare, under T3 Code's MCP server
/// (`mcp__t3-code__html_render`, `t3-code.html_render`), or behind OpenCode's thread prefix.
pub fn is_tool(name: &str) -> bool {
    let label = name.trim();
    let lower = label.to_ascii_lowercase();
    let label = ["completed", "complete"]
        .iter()
        .find_map(|word| lower.strip_suffix(word).filter(|head| head.ends_with(char::is_whitespace)).map(|head| label[..head.len()].trim_end()))
        .unwrap_or(label);
    let Some(head) = label.strip_suffix("html_render") else { return false };
    if head.is_empty() {
        return true;
    }
    let head = head.to_ascii_lowercase();
    if let Some(server) = head.strip_prefix("mcp__") {
        return server.strip_suffix("__").is_some_and(|server| T3_SERVERS.contains(&server));
    }
    if head.starts_with("t3-code-") && head.ends_with('_') {
        return true;
    }
    let server = head.strip_prefix("mcp").map_or(head.as_str(), |rest| rest.trim_start_matches(['-', '_']));
    T3_SERVERS.contains(&server.trim_end_matches(['.', ':', '/', '_', '-', ' ', '\u{b7}']))
}

fn read_result(value: &Value, depth: usize) -> (Option<Value>, bool) {
    read_field(value, "htmlRender", depth)
}

/// toolOutput.ts readResult: the result data's `key` (null when it has none) and whether it
/// reports a failure, through JSON text, content blocks and MCP envelopes.
pub(crate) fn read_field(value: &Value, key: &str, depth: usize) -> (Option<Value>, bool) {
    if depth > 8 {
        return (None, false);
    }
    match value {
        Value::String(raw) => serde_json::from_str::<Value>(raw).map_or((None, false), |parsed| read_field(&parsed, key, depth + 1)),
        Value::Array(blocks) => {
            let (mut data, mut failed) = (None, false);
            for block in blocks {
                let text = &block["text"];
                let (found, broke) = read_field(if text.is_object() && !text["text"].is_null() { &text["text"] } else { text }, key, depth + 1);
                data = data.or(found);
                failed |= broke;
            }
            (data, failed)
        }
        Value::Object(fields) => {
            let failed = value["isError"] == true || value["is_error"] == true || data::s(value, "_tag") == "OrchestratorMcpFailure" || !value["error"].is_null();
            match fields.get("structuredContent").filter(|c| !c.is_null()).or_else(|| fields.get("content")) {
                Some(content) => {
                    let (found, broke) = read_field(content, key, depth + 1);
                    (found, failed || broke)
                }
                None => (Some(value[key].clone()), failed),
            }
        }
        _ => (None, false),
    }
}

/// htmlRenderFromToolItem + readHtmlRenderReference: a finished `html_render` call's page, as
/// its attachment id and title.
pub fn reference(item: &Value) -> Option<(String, String)> {
    if data::s(item, "type") != "dynamic_tool" || data::s(item, "status") != "completed" || !is_tool(data::s(item, "toolName")) {
        return None;
    }
    let (page, failed) = read_result(&item["output"], 0);
    let page = page.filter(|_| !failed)?;
    let id = page["attachmentId"].as_str().filter(|id| (1..=256).contains(&id.len()))?;
    page["height"].as_f64()?;
    let title: String = page["title"].as_str()?.trim().chars().take(200).collect();
    Some((id.to_owned(), if title.is_empty() { "HTML".to_owned() } else { title }))
}

/// HtmlRenderTimelineRow: the page inline at its frame height, with "Open full screen".
pub fn row_view(app: &App, item: &Value, t: &T) -> Option<El> {
    let (id, title) = reference(item)?;
    let page = read_result(&item["output"], 0).0?;
    Some(crate::panel::html_frame::view(app, &id, &title, &page, t))
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.html_render.pending else { return false };
    let Some(net) = s.src.net() else { return false };
    let Some(reply) = net.take_reply(id) else { return false };
    let base = net.base_url().trim_end_matches('/').to_string();
    s.ui.chat.details.html_render.pending = None;
    match reply.ok().and_then(|v| v["relativeUrl"].as_str().map(|path| format!("{base}{path}"))) {
        Some(url) => crate::panel::open_preview(s, &url),
        None => unavailable(s),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{file_name, is_tool, reference};
    use serde_json::json;

    #[test]
    fn html_render_names_resolve_like_t3_tool_ids() {
        for name in ["html_render", "mcp__t3-code__html_render", "mcp__T3Code__html_render", "t3-code.html_render", "t3_code_direct/html_render", "mcp_t3code__html_render", "t3-code-thread_9_html_render", "html_render completed"] {
            assert!(is_tool(name), "{name}");
        }
        for name in ["mcp__other__html_render", "mcp__t3__html_render", "html_preview", "my_html_render", "t3-code.foo.html_render", "HTML_RENDER"] {
            assert!(!is_tool(name), "{name}");
        }
    }

    #[test]
    fn a_finished_render_reads_its_page_from_the_tool_output() {
        let page = json!({"htmlRender": {"attachmentId": "a1", "title": "  Q3 revenue  ", "height": 420}});
        let call = |output: serde_json::Value, status: &str| json!({"type": "dynamic_tool", "toolName": "mcp__t3-code__html_render", "status": status, "output": output});
        let expected = Some(("a1".to_owned(), "Q3 revenue".to_owned()));
        assert_eq!(reference(&call(json!({"structuredContent": page.clone()}), "completed")), expected);
        assert_eq!(reference(&call(json!([{"type": "text", "text": page.to_string()}]), "completed")), expected);
        assert_eq!(reference(&call(json!({"content": [{"type": "text", "text": page.to_string()}]}), "completed")), expected);
        assert_eq!(reference(&call(json!({"structuredContent": page.clone(), "isError": true}), "completed")), None);
        assert_eq!(reference(&call(json!({"structuredContent": page.clone()}), "running")), None);
        assert_eq!(reference(&call(json!({"structuredContent": {"htmlRender": {"attachmentId": "a1", "title": "x"}}}), "completed")), None);
        let untitled = json!({"structuredContent": {"htmlRender": {"attachmentId": "a1", "title": " ", "height": 80}}});
        assert_eq!(reference(&call(untitled, "completed")).map(|(_, title)| title).as_deref(), Some("HTML"));
    }

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(file_name("Q3 revenue: by region"), "Q3 revenue by region.html");
        assert_eq!(file_name("  a/b\\c  "), "a b c.html");
        assert_eq!(file_name("***"), "Page.html");
        assert_eq!(file_name(&"x".repeat(200)).len(), 125);
    }
}
