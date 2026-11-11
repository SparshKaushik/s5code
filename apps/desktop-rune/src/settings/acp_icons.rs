//! ACP Registry agent icons (web `AcpRegistryAgentIcon`): the allowlisted CDN's SVGs download off
//! the UI thread once per URL and draw as monochrome icons in the surrounding ink, as web's
//! alpha filter recolours them. Until one loads, or when it fails, the ACP glyph stands in.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use rune::{El, Icon, icon};
use serde_json::Value;

use crate::App;
use crate::theme::T;

const MAX_BYTES: u64 = 512 * 1024;
const MAX_IN_FLIGHT: usize = 6;

type Slot = Arc<Mutex<Option<Option<&'static Icon>>>>;

enum Entry {
    Loading(Slot),
    Ready(&'static Icon),
    Missing,
}

#[derive(Default)]
pub struct AcpIcons {
    entries: HashMap<String, Entry>,
}

/// `resolveOfficialAcpRegistryIconUrl`: https on the registry CDN, nothing else.
pub fn official(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://cdn.agentclientprotocol.com/")?;
    (!rest.is_empty() && !url.contains('@')).then_some(url)
}

/// `resolveProviderInstanceAcpRegistryIconUrl`: the saved icon, else the agent's CDN icon; a
/// local command has none.
pub fn instance_url(instance: &Value) -> Option<String> {
    let config = &instance["config"];
    if config["source"] == "local" {
        return None;
    }
    if let Some(url) = config["registryIconUrl"].as_str().map(str::trim).and_then(official) {
        return Some(url.to_owned());
    }
    let agent = config["agentId"].as_str()?.trim();
    let valid = agent.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && agent.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-'));
    valid.then(|| format!("https://cdn.agentclientprotocol.com/registry/v1/latest/{agent}.svg"))
}

/// An SVG document as a Rune icon: its elements inside a viewBox anchored at the origin.
fn parse(svg: &str) -> Option<(f32, String)> {
    let open = svg.find("<svg")?;
    let head_end = open + svg[open..].find('>')?;
    let head = &svg[open..head_end];
    let close = svg.rfind("</svg>")?;
    let body = svg.get(head_end + 1..close)?.trim();
    let view = head.split("viewBox=\"").nth(1)?.split('"').next()?;
    let n: Vec<f32> = view.split([' ', ',']).filter(|p| !p.is_empty()).map(|p| p.parse().ok()).collect::<Option<_>>()?;
    let &[x, y, w, h] = n.as_slice() else { return None };
    (x == 0.0 && y == 0.0 && w > 0.0 && h > 0.0 && !body.is_empty()).then(|| (w.max(h), body.to_owned()))
}

fn fetch(url: &str) -> Option<&'static Icon> {
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(20)).call().ok()?;
    let mut bytes = Vec::new();
    use std::io::Read;
    response.into_reader().take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let (view, body) = parse(std::str::from_utf8(&bytes).ok()?)?;
    // One per registry agent for the run: Rune keys rasterized icons by address.
    Some(Box::leak(Box::new(Icon { name: "acp-registry-agent", view, body: Box::leak(body.into_boxed_str()) })))
}

/// The registry icons on screen: search results and the registry instances' saved icons.
fn wanted(s: &App) -> Vec<String> {
    let mut urls: Vec<String> = super::add_provider::agents(s).iter().filter_map(|a| a["icon"].as_str()).filter_map(official).map(str::to_owned).collect();
    let instances = super::server_settings(s).get("providerInstances").and_then(Value::as_object);
    urls.extend(instances.into_iter().flat_map(|m| m.values()).filter(|i| i["driver"] == "acpRegistry").filter_map(instance_url));
    urls
}

pub fn poll(s: &mut App) -> bool {
    let urls = wanted(s);
    let waker = s.ui.panel.waker.clone();
    let icons = &mut s.ui.settings.acp_icons;
    let mut changed = false;
    let mut in_flight = icons.entries.values().filter(|e| matches!(e, Entry::Loading(_))).count();
    for url in urls {
        if in_flight >= MAX_IN_FLIGHT || icons.entries.contains_key(&url) {
            continue;
        }
        let slot: Slot = Arc::new(Mutex::new(None));
        let (out, target, waker) = (slot.clone(), url.clone(), waker.clone());
        std::thread::spawn(move || {
            let fetched = fetch(&target);
            *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(fetched);
            if let Some(waker) = waker {
                waker();
            }
        });
        icons.entries.insert(url, Entry::Loading(slot));
        in_flight += 1;
    }
    for entry in icons.entries.values_mut() {
        let fetched = match entry {
            Entry::Loading(slot) => slot.lock().unwrap_or_else(|e| e.into_inner()).take(),
            _ => None,
        };
        if let Some(fetched) = fetched {
            *entry = fetched.map_or(Entry::Missing, Entry::Ready);
            changed = true;
        }
    }
    changed
}

/// The agent's registry icon in the muted ink web's `AcpRegistryAgentIcon` uses, or the ACP glyph
/// while it loads or when there is none.
pub fn mark(s: &App, t: &T, url: Option<&str>, size: f32) -> El {
    match url.and_then(official).and_then(|u| s.ui.settings.acp_icons.entries.get(u)) {
        Some(Entry::Ready(glyph)) => icon(glyph, size).color(t.c.muted_foreground),
        _ => crate::chat::composer::provider_icon(t, "acpRegistry", size),
    }
}

/// A provider instance's mark: its registry agent's icon for an ACP Registry instance.
pub fn instance_mark(s: &App, t: &T, driver: &str, instance: Option<&Value>, size: f32) -> El {
    if driver != "acpRegistry" {
        return crate::chat::composer::provider_icon(t, driver, size);
    }
    mark(s, t, instance.and_then(instance_url).as_deref(), size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_svgs_become_icons_and_only_the_cdn_is_fetched() {
        let amp = "<svg width=\"16\" height=\"16\" viewBox=\"0 0 28 28\" fill=\"none\">\n<path d=\"M1 1H2Z\" fill=\"currentColor\"/>\n</svg>\n";
        assert_eq!(parse(amp), Some((28.0, "<path d=\"M1 1H2Z\" fill=\"currentColor\"/>".to_owned())));
        assert_eq!(parse("<svg viewBox=\"0,0,16,24\"><rect width=\"4\" height=\"4\"/></svg>").map(|p| p.0), Some(24.0));
        assert_eq!(parse("<svg viewBox=\"2 2 16 16\"><rect/></svg>"), None, "an offset viewBox would draw shifted");
        assert_eq!(parse("<svg viewBox=\"0 0 16 16\"></svg>"), None);
        assert!(official("https://cdn.agentclientprotocol.com/registry/v1/latest/amp-acp.svg").is_some());
        assert!(official("http://cdn.agentclientprotocol.com/x.svg").is_none());
        assert!(official("https://cdn.agentclientprotocol.com.evil.dev/x.svg").is_none());
        assert!(official("https://cdn.agentclientprotocol.com/").is_none());
        let instance = |config: Value| serde_json::json!({"driver": "acpRegistry", "config": config});
        assert_eq!(instance_url(&instance(serde_json::json!({"agentId": "amp-acp"}))).as_deref(), Some("https://cdn.agentclientprotocol.com/registry/v1/latest/amp-acp.svg"));
        assert_eq!(
            instance_url(&instance(serde_json::json!({"agentId": "amp-acp", "registryIconUrl": "https://cdn.agentclientprotocol.com/i/amp.svg"}))).as_deref(),
            Some("https://cdn.agentclientprotocol.com/i/amp.svg")
        );
        assert_eq!(instance_url(&instance(serde_json::json!({"agentId": "Amp ACP"}))), None);
        assert_eq!(instance_url(&instance(serde_json::json!({"source": "local", "agentId": "amp-acp"}))), None);
    }
}
