//! ThreadServersPanel: local servers the thread's terminals and agent started. Hidden while none run.

use rune::{Cursor, El, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, data, icons, kit};

/// The browser panel's stream too; it is shared by route, so neither side unwatches it for the
/// other while a thread card shows.
const SERVERS: &str = "subscribeDiscoveredLocalServers";
const PAGE: usize = 4;

#[derive(Default)]
pub struct State {
    /// The thread whose list was paged, and how many rows it shows.
    shown: Option<(String, usize)>,
    /// The `preview.stopLocalServer` in flight and the server (`pid:port`) it stops.
    stopping: Option<(u64, String)>,
    /// What fixtures and scenes list in place of the live stream.
    pub fixture: Option<Value>,
}

/// useThreadDiscoveredPorts: the servers the thread's terminals or agent started.
pub fn for_thread<'a>(list: &'a Value, thread: &str) -> Vec<&'a Value> {
    list["servers"].as_array().into_iter().flatten().filter(|p| p["terminal"]["threadId"] == thread || p["agent"]["threadId"] == thread).collect()
}

fn list(s: &App) -> Option<&Value> {
    match &s.src {
        data::Source::Live(net) => net.resource(SERVERS, json!({})),
        _ => s.ui.chat.details.servers.fixture.as_ref(),
    }
}

pub fn watch(s: &mut App) {
    if super::shows(s, "servers")
        && let Some(net) = s.src.net()
    {
        net.watch_resource(SERVERS, json!({}));
    }
}

/// PreviewStopLocalServerError's message, else the reply's own.
pub fn stop_failure(error: &Value) -> String {
    match super::tagged(error, "PreviewStopLocalServerError").map(|e| data::s(e, "reason")) {
        Some("not-found") => "This server is no longer running.".into(),
        Some("shared-group") => "This server shares a process group with T3 Code, an agent, or a terminal shell, so it can't be stopped on its own.".into(),
        Some("unsupported") => "Stopping servers is not supported on this environment's platform.".into(),
        Some("signal-failed") => "The server's processes could not be signaled.".into(),
        _ => crate::shell::palette::error_message(error, "An error occurred."),
    }
}

fn stop(s: &mut App, pid: u64, port: u64, key: String) {
    if s.ui.chat.details.servers.stopping.is_some() {
        return;
    }
    let Some(net) = s.src.net() else { return };
    let id = net.request("preview.stopLocalServer", json!({"pid": pid, "port": port}));
    s.ui.chat.details.servers.stopping = Some((id, key));
}

pub fn poll(s: &mut App) -> bool {
    let Some(id) = s.ui.chat.details.servers.stopping.as_ref().map(|p| p.0) else { return false };
    let Some(reply) = s.src.net().and_then(|net| net.take_reply(id)) else { return false };
    s.ui.chat.details.servers.stopping = None;
    if let Err(e) = reply {
        crate::shell::palette::toast_error_brief(s, "Could not stop server", Some(&stop_failure(&e)));
    }
    true
}

/// The "Show N more" / "Show less" label, and the row count the press shows next.
pub fn paging(total: usize, shown: usize) -> Option<(String, usize)> {
    if total <= PAGE {
        return None;
    }
    let hidden = total.saturating_sub(shown);
    Some(if hidden > 0 { (format!("Show {} more", hidden.min(PAGE)), shown + PAGE) } else { ("Show less".into(), PAGE) })
}

pub fn view(s: &App, t: &T, thread: &str, divided: bool) -> Option<El> {
    let servers = list(s).map(|l| for_thread(l, thread)).unwrap_or_default();
    if servers.is_empty() {
        return None;
    }
    let ui = &s.ui.chat.details.servers;
    let shown = ui.shown.as_ref().filter(|(id, _)| id == thread).map_or(PAGE, |(_, n)| *n);
    let can_open = super::grants(s, "preview:operate");
    let supports_stop = s.state().config.as_ref().is_some_and(|c| c["environment"]["capabilities"]["localServerStop"] == true);
    let can_stop = supports_stop && super::grants(s, "terminal:operate");
    let stopping = ui.stopping.as_ref().map(|p| p.1.as_str());
    let muted = t.c.muted_foreground;
    let mut rows = col().w_full();
    for server in servers.iter().take(shown) {
        let port = server["port"].as_u64().unwrap_or(0);
        let pid = server["pid"].as_u64();
        let key = format!("{}:{port}", pid.map_or_else(|| "null".to_string(), |p| p.to_string()));
        let source = if server["terminal"].is_null() { "Agent" } else { "Terminal" };
        let detail = match server["processName"].as_str().filter(|n| !n.is_empty()) {
            Some(name) => format!("{source} · {name}"),
            None => source.to_string(),
        };
        let label = format!("localhost:{port}");
        let url = data::s(server, "url").to_string();
        let mut open = row()
            .key(format!("details-server-{key}"))
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .min_h(32.0)
            .py(3.0)
            .pl(11.0)
            .pr(10.0)
            .gap(10.0)
            .center_y()
            .role("button")
            .label(label.clone())
            .help("Open in the browser panel.")
            .child(icon(&icons::GLOBE, 16.0).color(muted))
            .child(
                col()
                    .grow(1.0)
                    .shrink()
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(label.clone()).font(theme::SM_MEDIUM.lh(16.0)).color(t.c.foreground.alpha_mul(0.85)).truncate())
                    .child(text(detail).font(theme::TEXT_2XS).color(muted).truncate()),
            );
        open = if can_open { open.cursor(Cursor::Pointer).on_press(move |s: &mut App| crate::panel::open_preview(s, &url)) } else { open.opacity(0.64) };
        let mut line = row().key(format!("details-server-row-{key}")).w_full().center_y().radius(theme::layout::CONTROL_RADIUS).hover(t.c.accent).hover_group().child(open);
        if let (true, Some(pid)) = (can_stop, pid) {
            let busy = stopping == Some(key.as_str());
            let glyph = if busy { kit::Spinner::new().size("xs").build(&kit::theme(t)) } else { el().size(9.0, 9.0).radius(1.5).fill(t.c.destructive_foreground) };
            let mut button = el()
                .key(format!("details-server-stop-{key}"))
                .size(24.0, 24.0)
                .mr(4.0)
                .center()
                .radius(6.0)
                .role("button")
                .label(format!("Stop {label}"))
                .help("Stop server")
                .group_opacity(0.0, 1.0)
                .child(glyph);
            button = if stopping.is_some() {
                button.opacity(0.64)
            } else {
                button.hover(t.c.destructive.alpha_mul(0.1)).cursor(Cursor::Pointer).on_press(move |s: &mut App| stop(s, pid, port, key.clone()))
            };
            line = line.child(button);
        }
        rows = rows.child(line);
    }
    if let Some((label, next)) = paging(servers.len(), shown) {
        let thread = thread.to_string();
        let more = row()
            .key("details-servers-more")
            .w_full()
            .h(32.0)
            .pl(11.0)
            .gap(10.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .role("button")
            .label(label.clone())
            .child(icon(if next > shown { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_UP }, 16.0).color(muted))
            .child(text(label).font(super::ROW).color(muted))
            .on_press(move |s: &mut App| s.ui.chat.details.servers.shown = Some((thread.clone(), next)));
        rows = rows.child(more);
    }
    Some(super::section(t, "Servers", None, divided, rows))
}


/// Scene: six servers on the thread, a page of four with "Show 2 more", and Stop available.
pub fn scene(s: &mut App) {
    let Some(thread) = super::scene_thread(s) else { return };
    let server = |port: u64, pid: Option<u64>, name: Option<&str>, terminal: bool| {
        let owner = json!({"threadId": thread, "terminalId": "term-1"});
        json!({"host": "localhost", "port": port, "url": format!("http://localhost:{port}/"), "processName": name, "pid": pid,
            "terminal": if terminal { owner } else { Value::Null }, "agent": if terminal { Value::Null } else { json!({"threadId": thread}) }})
    };
    s.ui.chat.details.servers.fixture = Some(json!({"servers": [
        server(5173, Some(4101), Some("node"), true),
        server(3000, Some(4102), Some("next-server"), false),
        server(8080, None, None, true),
        server(6006, Some(4104), Some("storybook"), true),
        server(9229, Some(4105), Some("node"), false),
        server(4000, Some(4106), Some("ruby"), true),
    ]}));
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        config["environment"]["capabilities"]["localServerStop"] = json!(true);
    }
}

#[cfg(test)]
mod tests {
    use super::{for_thread, paging, stop_failure};
    use serde_json::json;

    #[test]
    fn a_thread_lists_the_servers_its_terminals_and_agent_started() {
        let list = json!({"servers": [
            {"port": 3000, "terminal": {"threadId": "t1", "terminalId": "a"}},
            {"port": 3001, "terminal": null, "agent": {"threadId": "t1"}},
            {"port": 3002, "terminal": {"threadId": "t2", "terminalId": "b"}},
            {"port": 3003, "terminal": null},
        ]});
        let ports: Vec<u64> = for_thread(&list, "t1").iter().map(|p| p["port"].as_u64().unwrap()).collect();
        assert_eq!(ports, [3000, 3001]);
        assert!(for_thread(&json!({}), "t1").is_empty());
    }

    #[test]
    fn the_list_pages_by_four_then_folds_back() {
        assert_eq!(paging(4, 4), None);
        assert_eq!(paging(10, 4), Some(("Show 4 more".into(), 8)));
        assert_eq!(paging(10, 8), Some(("Show 2 more".into(), 12)));
        assert_eq!(paging(10, 12), Some(("Show less".into(), 4)));
    }

    #[test]
    fn a_stop_failure_reads_its_reason() {
        let tagged = json!({"_tag": "Fail", "error": {"_tag": "PreviewStopLocalServerError", "reason": "not-found"}});
        assert_eq!(stop_failure(&tagged), "This server is no longer running.");
        assert_eq!(stop_failure(&json!({"message": "boom"})), "boom");
    }
}
