//! HtmlRenderFrame's "Open full screen" (HtmlRenderDialog). Rune draws no HTML in-process, so the
//! page opens in the browser panel, whose own title, zoom and close stand in for the dialog's.

use serde_json::json;

use crate::App;

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
    use super::file_name;

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(file_name("Q3 revenue: by region"), "Q3 revenue by region.html");
        assert_eq!(file_name("  a/b\\c  "), "a b c.html");
        assert_eq!(file_name("***"), "Page.html");
        assert_eq!(file_name(&"x".repeat(200)).len(), 125);
    }
}
