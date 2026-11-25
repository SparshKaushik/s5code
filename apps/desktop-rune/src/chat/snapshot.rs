//! SnapShotAttachmentDetails: a snapshot's app and window over its picture, and its accessibility
//! data (SnapShotContentsButton) in a popover; the lightbox shows the data in place of the picture.

use rune::{Align, Color, Cursor, El, Side, col, el, icon, row, text};
use serde_json::Value;

use crate::data::s;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, icons};

#[derive(Default)]
pub struct State {
    /// The contents button whose popover is open, and the snapshot it describes.
    pub(crate) open: Option<(String, Value)>,
    /// The lightbox image showing its accessibility data instead.
    pub lightbox: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct Details {
    pub content: String,
    pub json: bool,
}

pub fn is_snapshot(source: &Value) -> bool {
    source["kind"] == "snap-shot"
}

/// snapShotAccessibilityDetails: an element tree as JSON, else the flat or accessible text.
pub fn details(source: &Value) -> Option<Details> {
    let tree = &source["accessibility"];
    if tree["format"] == "element-tree" {
        return Some(Details { content: serde_json::to_string_pretty(tree).unwrap_or_default(), json: true });
    }
    let text = if tree["format"] == "flat-text" { s(tree, "text") } else { s(source, "accessibleText") }.trim();
    (!text.is_empty()).then(|| Details { content: text.to_owned(), json: false })
}

/// snapShotIncludesAccessibility.
fn includes(source: &Value) -> bool {
    !source["accessibility"].is_null() || !s(source, "accessibleText").trim().is_empty()
}

/// A captured editor window with its element tree, for scenes.
pub fn scene_source() -> Value {
    serde_json::json!({
        "kind": "snap-shot", "capturedAt": "2026-01-01T00:00:00Z", "appName": "Safari", "windowTitle": "Release notes — T3 Code",
        "accessibility": {"format": "element-tree", "root": {"role": "window", "name": "Release notes — T3 Code", "children": [
            {"role": "heading", "name": "What's new", "level": 1},
            {"role": "link", "name": "Download for macOS", "value": "https://t3.codes/download"},
            {"role": "button", "name": "Copy install command", "enabled": true}]}},
    })
}

/// A 640×400 gradient standing in for a capture.
pub fn scene_picture() -> image::RgbaImage {
    image::RgbaImage::from_fn(640, 400, |x, y| image::Rgba([40 + (x * 120 / 640) as u8, 60 + (y * 100 / 400) as u8, 140, 255]))
}

/// SnapShotContentsButton: a micro overlay button opening the popover above itself.
pub fn contents_button(app: &App, source: &Value, key: String) -> El {
    let has = includes(source);
    let open = app.ui.chat.snapshot.open.as_ref().is_some_and(|(k, _)| *k == key);
    let (source, at) = (source.clone(), key.clone());
    el().key(key)
        .size(18.0, 18.0)
        .center()
        .radius(radius::SM)
        .fill(Color::BLACK.alpha(0.35))
        .hover(Color::BLACK.alpha(0.6))
        .cursor(Cursor::Pointer)
        .role("button")
        .expanded(open)
        .label(if has { "View accessibility data" } else { "No accessibility data" })
        .help(if has { "Accessibility data" } else { "No accessibility data" })
        .child(icon(if has { &icons::TEXT } else { &icons::IMAGE }, 12.0).color(Color::WHITE))
        .on_press(move |app: &mut App| {
            let state = &mut app.ui.chat.snapshot;
            state.open = if open { None } else { Some((at.clone(), source.clone())) };
        })
}

/// The app and window along the picture's bottom edge, over a fade to black.
pub fn overlay(app: &App, source: &Value, key: String) -> El {
    let name = s(source, "appName");
    let initial = name.chars().next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    let window = Some(s(source, "windowTitle")).filter(|w| !w.is_empty()).unwrap_or("Captured window");
    let badge = el()
        .size(28.0, 28.0)
        .center()
        .radius(radius::MD)
        .fill(Color::WHITE.alpha(0.2))
        .child(text(initial).font(theme::weight(theme::TEXT_3XS, theme::MEDIUM)).color(Color::WHITE));
    let lines = col()
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .child(
            row()
                .min_w(0.0)
                .gap(6.0)
                .center_y()
                .child(text(name.to_owned()).font(theme::weight(theme::TEXT_2XS, theme::MEDIUM).lh(14.0)).color(Color::WHITE).truncate().shrink().min_w(0.0))
                .child(contents_button(app, source, key)),
        )
        .child(text(window.to_owned()).font(theme::TEXT_3XS.lh(14.0)).color(Color::WHITE.alpha(0.7)).truncate());
    row()
        .pin(Some(0.0), None, Some(0.0), Some(0.0))
        .min_w(0.0)
        .gap(6.0)
        .center_y()
        .px(10.0)
        .pt(24.0)
        .pb(8.0)
        .gradient(Color::BLACK.alpha(0.0), Color::BLACK.alpha(0.85))
        .child(badge)
        .child(lines)
}

/// SnapShotAccessibilityData: JSON highlighted as a code block, text as it reads.
pub fn data_view(app: &App, t: &T, details: &Details, key: &str) -> El {
    if details.json {
        let fence = "`".repeat(details.content.split(|c: char| c != '`').map(str::len).max().unwrap_or(0).max(2) + 1);
        return crate::chat::timeline::markdown::view(app, &format!("{fence}json\n{}\n{fence}", details.content), key, t);
    }
    text(details.content.clone()).font(theme::mono(theme::TEXT_2XS)).color(t.c.foreground).wrap().selectable(true)
}

/// The open contents popover, above its button.
pub fn popover(app: &App, t: &T) -> Option<El> {
    let (key, source) = app.ui.chat.snapshot.open.as_ref()?;
    let note = |words: &str| {
        el().w_full()
            .padding(10.0)
            .radius(radius::MD)
            .border(1.0, t.c.border.alpha_mul(0.7))
            .fill(t.c.muted.alpha_mul(0.45))
            .child(text(words.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap())
    };
    let body = match details(source) {
        Some(d) => el()
            .w_full()
            .max_h(256.0)
            .padding(10.0)
            .radius(radius::MD)
            .border(1.0, t.c.border.alpha_mul(0.7))
            .fill(t.c.muted.alpha_mul(0.45))
            .child(data_view(app, t, &d, &format!("{key}-data")))
            .scroll(rune::ScrollSpec { stick_bottom: false }),
        None if includes(source) => note("Structured accessibility elements were included, but they have no readable names or values."),
        None => note("The app or capture backend did not provide verified accessibility data."),
    };
    let close = |app: &mut App| app.ui.chat.snapshot.open = None;
    Some(
        col()
            .key("snapshot-contents")
            .w(320.0)
            .gap(8.0)
            .padding(12.0)
            .radius(radius::LG)
            .fill(t.c.popover)
            .border(1.0, t.c.border)
            .shadow(8.0, 24.0, Color::BLACK.alpha(0.3))
            .role("dialog")
            .label("Accessibility data")
            .child(text("Accessibility data").font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(body)
            .anchored(key.clone(), Side::Top, Align::Center, 6.0)
            .modal(true, close),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accessibility_details_prefer_the_tree_then_flat_then_accessible_text() {
        let tree = json!({"kind": "snap-shot", "accessibility": {"format": "element-tree", "root": {"role": "window"}}, "accessibleText": "ignored"});
        let d = details(&tree).unwrap();
        assert!(d.json && d.content.contains("\"role\": \"window\""));
        let flat = json!({"accessibility": {"format": "flat-text", "text": "  Body  "}, "accessibleText": "ignored"});
        assert_eq!(details(&flat), Some(Details { content: "Body".into(), json: false }));
        assert_eq!(details(&json!({"accessibleText": "Docs"})).map(|d| d.content), Some("Docs".into()));
        // Elements with nothing readable still count as included.
        let empty = json!({"accessibility": {"format": "flat-text", "text": " "}});
        assert!(details(&empty).is_none() && includes(&empty));
        assert!(!includes(&json!({"accessibleText": " "})));
    }
}
