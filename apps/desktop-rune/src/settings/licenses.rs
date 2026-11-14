//! Web `OpenSourceLicensesPanel`: the environment's `third-party-licenses.json` (the manifest the
//! web build emits next to the app), searchable, one notice open at a time.
use std::sync::{Arc, Mutex};

use crate::kit::{Button, Input};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, row, text};
use serde_json::Value;

pub const PATH: &str = "open-source-licenses";
const MAX_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    bundles: Vec<String>,
    kind: String,
    license: String,
    name: String,
    notice: String,
    source_url: Option<String>,
    version: Option<String>,
}

type Outcome = Result<Vec<Entry>, String>;

#[derive(Default)]
pub struct Ui {
    pending: Option<Arc<Mutex<Option<Outcome>>>>,
    result: Option<Outcome>,
    query: TextEdit,
    search_open: bool,
    open: Option<String>,
}

fn is_http_url(value: &str) -> bool {
    s5_net::url::Url::parse(value).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

fn decode_entry(value: &Value, index: usize) -> Result<Entry, String> {
    let n = index + 1;
    let Some(object) = value.as_object() else { return Err(format!("License entry {n} is not an object.")) };
    let strings = |v: &Value| v.as_array().map(|a| a.iter().map(|s| s.as_str().map(str::to_owned)).collect::<Option<Vec<_>>>());
    let optional = |v: &Value, valid: &dyn Fn(&str) -> bool| match v {
        Value::Null => Some(None),
        Value::String(s) if valid(s.as_str()) => Some(Some(s.clone())),
        _ => None,
    };
    static NULL: Value = Value::Null;
    let field = |key: &str| object.get(key).unwrap_or(&NULL);
    let shape = (|| {
        let bundles = strings(field("bundles"))??;
        let kind = field("kind").as_str().filter(|k| matches!(*k, "custom" | "package"))?;
        Some(Entry {
            kind: kind.to_owned(),
            license: field("license").as_str()?.to_owned(),
            name: field("name").as_str()?.to_owned(),
            notice: field("noticeText").as_str()?.to_owned(),
            source_url: optional(field("sourceUrl"), &is_http_url)?,
            version: optional(field("version"), &|_| true)?,
            bundles: (!bundles.is_empty()).then_some(bundles)?,
        })
    })();
    shape.ok_or_else(|| format!("License entry {n} has an invalid shape."))
}

/// `encodeURIComponent`: everything but `A-Z a-z 0-9 - _ . ! ~ * ' ( )` percent-encoded as UTF-8.
fn encode_uri_component(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// `thirdPartyLicenseEntryKey`.
fn key(entry: &Entry) -> String {
    encode_uri_component(&serde_json::json!([entry.kind, entry.name, entry.version]).to_string())
}

/// `decodeThirdPartyLicenseManifest`.
pub fn decode(value: &Value) -> Outcome {
    let (Some(1), Some(entries)) = (value["schemaVersion"].as_i64(), value["entries"].as_array()) else {
        return Err("The open-source license manifest has an unsupported format.".into());
    };
    let entries = entries.iter().enumerate().map(|(i, e)| decode_entry(e, i)).collect::<Result<Vec<_>, _>>()?;
    let mut seen = std::collections::HashSet::new();
    for entry in &entries {
        let key = key(entry);
        if !seen.insert(key.clone()) {
            return Err(format!("The open-source license manifest contains a duplicate entry: {key}"));
        }
    }
    Ok(entries)
}

/// `filterThirdPartyLicenseEntries`: every whitespace-separated term in name, version, license
/// or a bundle.
fn filter<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let query = query.trim().to_lowercase();
    let terms: Vec<&str> = query.split_whitespace().collect();
    entries
        .iter()
        .filter(|entry| {
            let mut fields = vec![entry.name.as_str()];
            fields.extend(entry.version.as_deref());
            fields.push(entry.license.as_str());
            fields.extend(entry.bundles.iter().map(String::as_str));
            let searchable = fields.join(" ").to_lowercase();
            terms.iter().all(|term| searchable.contains(term))
        })
        .collect()
}

/// `formatLicenseBundles`.
fn bundles(bundles: &[String]) -> String {
    let label = |b: &str| match b {
        "android" => "Android",
        "assets" => "Assets",
        "desktop" => "Desktop",
        "device-tools" => "Device tools",
        "ios" => "iOS",
        "mobile" => "Mobile",
        "server" => "Server",
        "web" => "Web",
        _ => "",
    };
    bundles.iter().map(|b| if label(b).is_empty() { b.clone() } else { label(b).to_owned() }).collect::<Vec<_>>().join(", ")
}

fn fetch(base: &str) -> Outcome {
    let url = format!("{}/third-party-licenses.json", base.trim_end_matches('/'));
    let response = match ureq::get(&url).timeout(std::time::Duration::from_secs(30)).call() {
        Ok(response) => response,
        Err(ureq::Error::Status(code, _)) => return Err(format!("The license manifest request failed with status {code}.")),
        Err(error) if error.to_string().contains("timed out") => return Err("The license manifest took too long to load.".into()),
        Err(error) => return Err(error.to_string()),
    };
    let mut bytes = Vec::new();
    use std::io::Read;
    response.into_reader().take(MAX_BYTES).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let body: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    decode(&body)
}

fn start(s: &mut App) {
    let Some(base) = s.src.net().map(|n| n.base_url().to_owned()) else { return };
    let slot = Arc::new(Mutex::new(None));
    let (out, waker) = (slot.clone(), s.ui.panel.waker.clone());
    std::thread::spawn(move || {
        *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(fetch(&base));
        if let Some(waker) = waker {
            waker();
        }
    });
    let ui = &mut s.ui.settings.licenses;
    ui.result = None;
    ui.pending = Some(slot);
}

/// Loads while the page is open (settings drops it on leaving); true when a load finished.
pub fn poll(s: &mut App) -> bool {
    if !matches!(&s.route, crate::Route::Settings(p) if p == PATH) {
        return false;
    }
    let ui = &mut s.ui.settings.licenses;
    if ui.pending.is_none() && ui.result.is_none() {
        start(s);
        return false;
    }
    let Some(done) = ui.pending.as_ref().and_then(|slot| slot.lock().unwrap_or_else(|e| e.into_inner()).take()) else { return false };
    ui.pending = None;
    ui.result = Some(done);
    true
}

/// Escape clears and closes the license search first, as web's field does.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.licenses;
    if !ui.search_open {
        return false;
    }
    ui.search_open = false;
    ui.query = TextEdit::default();
    true
}

fn count(filtered: usize, total: usize, t: &T) -> El {
    let label = if filtered == total { format!("{total} notices") } else { format!("{filtered} of {total}") };
    text(label).font(theme::TEXT_XS).color(t.c.muted_foreground)
}

fn header_action(s: &App, filtered: usize, total: usize, t: &T) -> El {
    let ui = &s.ui.settings.licenses;
    if !ui.search_open {
        return row().gap(6.0).center_y().child(count(filtered, total, t)).child(
            Button::new("Search open-source licenses")
                .icon(&icons::SEARCH)
                .size("icon-micro")
                .variant("ghost-muted")
                .build(t)
                .on_press(|s: &mut App| {
                    let ui = &mut s.ui.settings.licenses;
                    ui.search_open = true;
                    ui.query.focused = true;
                }),
        );
    }
    let field = Input::new(&ui.query)
        .placeholder("Search licenses")
        .size("sm")
        .on_edit(|s: &mut App| &mut s.ui.settings.licenses.query)
        .on_change(|s: &mut App, _| s.ui.settings.licenses.open = None)
        .build(t)
        .key("license-search")
        .single_line()
        .w(176.0);
    row().gap(8.0).center_y().child(count(filtered, total, t)).child(field)
}

fn notice_row(entry: &Entry, open: bool, t: &T) -> El {
    let key = key(entry);
    let toggle = key.clone();
    let mut name = row().grow(1.0).min_w(0.0).gap(8.0).center_y().child(
        text(entry.name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink(),
    );
    if let Some(version) = &entry.version {
        name = name.child(text(version.clone()).font(theme::mono(theme::TEXT_XS)).color(t.c.muted_foreground));
    }
    let trigger = row()
        .key(format!("license-{key}"))
        .grow(1.0)
        .min_w(0.0)
        .min_h(36.0)
        .px(16.0)
        .gap(10.0)
        .center_y()
        .focusable(true)
        .role("button")
        .expanded(open)
        .label(entry.name.clone())
        .on_press(move |s: &mut App| {
            let open = &mut s.ui.settings.licenses.open;
            *open = if open.as_deref() == Some(toggle.as_str()) { None } else { Some(toggle.clone()) };
        })
        .child(rune::icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.0).color(t.c.muted_foreground))
        .child(name)
        .child(
            text(format!("{} · {}", entry.license, bundles(&entry.bundles)))
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .truncate()
                .max_w(280.0),
        );
    let mut head = row().w_full().center_y().hover(t.c.muted.alpha_mul(0.35)).child(trigger);
    if let Some(url) = entry.source_url.clone() {
        head = head.child(
            Button::new(format!("View project source for {}", entry.name))
                .icon(&icons::EXTERNAL_LINK)
                .size("icon-micro")
                .variant("ghost-muted")
                .build(t)
                .mr(16.0)
                .on_press(move |s: &mut App| super::auth::open_url(s, &url)),
        );
    }
    let mut out = col().w_full().child(head);
    if open {
        out = out.child(
            col().w_full().px(40.0).pt(4.0).pb(16.0).child(
                text(entry.notice.clone())
                    .font(theme::mono(theme::TEXT_XS).lh(20.0))
                    .color(t.c.foreground.alpha_mul(0.8))
                    .wrap()
                    .selectable(true)
                    .max_w(620.0),
            ),
        );
    }
    out
}

pub fn view(s: &App, t: &T) -> El {
    let ui = &s.ui.settings.licenses;
    let (body, action) = match &ui.result {
        Some(Ok(entries)) => {
            let filtered = filter(entries, &ui.query.text);
            let action = header_action(s, filtered.len(), entries.len(), t);
            let body = if filtered.is_empty() {
                col().w_full().px(16.0).py(32.0).center().child(
                    text("No licenses match that search.").font(theme::TEXT_SM).color(t.c.muted_foreground),
                )
            } else {
                let mut list = col().w_full();
                for entry in filtered {
                    list = list.child(notice_row(entry, ui.open.as_deref() == Some(key(entry).as_str()), t));
                }
                list
            };
            (body, Some(action))
        }
        Some(Err(message)) => (
            col()
                .w_full()
                .px(16.0)
                .py(20.0)
                .gap(12.0)
                .items_start()
                .child(
                    col()
                        .gap(4.0)
                        .child(text("Open-source notices are unavailable").font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(message.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.8)).wrap()),
                )
                .child(Button::new("Try again").variant("outline").size("xs").build(t).on_press(start)),
            None,
        ),
        None => (
            row().w_full().px(16.0).py(20.0).child(text("Loading open-source notices…").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            None,
        ),
    };
    // Rows draw their own hover; the card's dividers would split every notice.
    let card = col()
        .w_full()
        .padding(1.0)
        .radius(12.0)
        .clip_rounded(12.0)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.card.alpha_mul(0.4))
        .child(body);
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .h(28.0)
                .px(16.0)
                .gap(8.0)
                .center_y()
                .child(
                    text("Third-party notices")
                        .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .children(action),
        )
        .child(card)
}

/// Headless scenes: a small manifest with one notice open.
pub fn scene(s: &mut App, search: Option<&str>) {
    let manifest = serde_json::json!({"schemaVersion": 1, "entries": [
        {"bundles": ["web", "desktop"], "kind": "package", "license": "MIT", "name": "react", "version": "19.2.0",
         "noticeText": "MIT License\n\nCopyright (c) Meta Platforms, Inc. and affiliates.\n\nPermission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the \"Software\"), to deal in the Software without restriction.",
         "sourceUrl": "https://github.com/facebook/react"},
        {"bundles": ["server"], "kind": "package", "license": "MIT", "name": "effect", "version": "4.0.0", "noticeText": "MIT License", "sourceUrl": "https://github.com/Effect-TS/effect"},
        {"bundles": ["assets"], "kind": "custom", "license": "OFL-1.1", "name": "Inter", "version": null, "noticeText": "SIL Open Font License", "sourceUrl": null},
        {"bundles": ["web"], "kind": "package", "license": "Apache-2.0", "name": "@pierre/diffs", "version": "1.0.0", "noticeText": "Apache License 2.0", "sourceUrl": null}
    ]});
    let entries = decode(&manifest).unwrap();
    let ui = &mut s.ui.settings.licenses;
    ui.open = Some(key(&entries[0]));
    ui.result = Some(Ok(entries));
    if let Some(query) = search {
        ui.search_open = true;
        ui.query = TextEdit::new(query);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn entry(name: &str, version: Option<&str>) -> Value {
        json!({"bundles": ["web"], "kind": "package", "license": "MIT", "name": name, "version": version, "noticeText": "MIT", "sourceUrl": null})
    }

    #[test]
    fn the_manifest_decodes_with_webs_checks_and_messages() {
        let ok = decode(&json!({"schemaVersion": 1, "entries": [entry("a", Some("1.0.0")), entry("a", None)]})).unwrap();
        assert_eq!(ok.len(), 2);
        assert_eq!(ok[1].version, None);
        assert_eq!(decode(&json!({"schemaVersion": 2, "entries": []})).unwrap_err(), "The open-source license manifest has an unsupported format.");
        assert_eq!(decode(&json!({"schemaVersion": 1, "entries": [1]})).unwrap_err(), "License entry 1 is not an object.");
        let mut bad = entry("b", None);
        bad["sourceUrl"] = json!("ftp://example.com");
        assert_eq!(decode(&json!({"schemaVersion": 1, "entries": [entry("a", None), bad]})).unwrap_err(), "License entry 2 has an invalid shape.");
        let mut empty = entry("c", None);
        empty["bundles"] = json!([]);
        assert!(decode(&json!({"schemaVersion": 1, "entries": [empty]})).is_err());
        assert_eq!(
            decode(&json!({"schemaVersion": 1, "entries": [entry("a b", Some("1")), entry("a b", Some("1"))]})).unwrap_err(),
            "The open-source license manifest contains a duplicate entry: %5B%22package%22%2C%22a%20b%22%2C%221%22%5D"
        );
    }

    #[test]
    fn search_needs_every_term_somewhere_in_the_entry() {
        let mut server = entry("effect", Some("4.0.0"));
        server["bundles"] = json!(["server", "desktop"]);
        server["license"] = json!("Apache-2.0");
        let entries = decode(&json!({"schemaVersion": 1, "entries": [entry("react", Some("19.0.0")), server]})).unwrap();
        let names = |q: &str| filter(&entries, q).iter().map(|e| e.name.clone()).collect::<Vec<_>>();
        assert_eq!(names("   "), ["react", "effect"]);
        assert_eq!(names("APACHE  server"), ["effect"]);
        assert_eq!(names("19.0 mit"), ["react"]);
        assert!(names("react server").is_empty());
    }

    #[test]
    fn bundle_labels_match_web() {
        assert_eq!(bundles(&["web".into(), "device-tools".into(), "ios".into(), "other".into()]), "Web, Device tools, iOS, other");
    }
}
