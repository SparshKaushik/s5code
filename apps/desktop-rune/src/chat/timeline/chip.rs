//! What a context chip shows past its label (ContextChip details, UserMessageContextChips,
//! composerContextPresentation): a tooltip on hover, and for the kinds with a body (terminal
//! output, a picked element, a review comment) a popover on click. A PR chip opens the PR, a
//! file mention opens the file and a thread chip opens the thread.

use rune::{Align, Color, El, Link, LinkPaint, ScrollSpec, Side, Tx, col, icon, row, text};
use serde_json::Value;

use crate::chat::composer::chips;
use crate::data::s;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::{App, Route};

/// The bubble's chip links, keyed `kind/id` for [`pressed`], with their tooltips.
pub fn links(shown: &[chips::Shown], records: &[Value]) -> Vec<Link> {
    shown
        .iter()
        .map(|c| {
            let record = records.iter().find(|r| s(r, "contextId") == c.id);
            let accent = chips::look(&c.kind, record).map(|(a, _)| a).unwrap_or(Color::TRANSPARENT);
            let link = Link::new(c.range.clone(), format!("{}/{}", c.kind, c.id)).role("button").hover(LinkPaint::new().underline(accent));
            match tooltip(&c.kind, &c.id, record) {
                Some(help) => link.help(help),
                None => link,
            }
        })
        .collect()
}

fn tooltip(kind: &str, id: &str, record: Option<&Value>) -> Option<String> {
    if kind == "mention" {
        return Some(id.to_owned());
    }
    let Some(r) = record else { return Some("This context is no longer available.".into()) };
    match kind {
        "review-comment" if r["pullRequest"].is_object() => {
            let pr = &r["pullRequest"];
            let state = match (s(pr, "state"), pr["isDraft"] == true) {
                ("merged", _) => "Merged",
                ("closed", _) => "Closed",
                (_, true) => "Draft",
                _ => "Open",
            };
            Some(format!("Pull request #{} · {state}\n{}\n{} → {}", pr["number"], s(pr, "title"), s(pr, "headBranch"), s(pr, "baseBranch")))
        }
        "thread" => Some("Open thread".into()),
        "environment" => Some("T3 Code environment".into()),
        "image" | "file" => Some(s(r, "name").to_owned()),
        _ => None,
    }
}

/// A bubble chip was clicked: act on it, or open its details over the message.
pub fn pressed(app: &mut App, item: &str, link: &str, records: &[Value], tx: &mut Tx) {
    let Some((kind, id)) = link.split_once('/') else { return };
    if kind == "mention" {
        crate::panel::open_file(app, id.to_owned());
        return;
    }
    let Some(record) = records.iter().find(|r| s(r, "contextId") == id) else { return };
    match kind {
        "review-comment" if record["pullRequest"].is_object() => {
            let _ = tx.open_url(s(&record["pullRequest"], "url"));
        }
        "thread" if !s(record, "threadId").is_empty() => app.navigate(Route::Thread(s(record, "threadId").to_owned())),
        "terminal" | "element" | "preview-annotation" | "review-comment" => {
            app.ui.chat.timeline.chip = Some((item.to_owned(), id.to_owned()));
        }
        _ => {}
    }
}

pub fn bubble_key(item: &str) -> String {
    format!("user-text-{item}")
}

/// The open bubble chip's popover, under its message.
pub fn overlay(app: &App, t: &T) -> Option<El> {
    let (item, id) = app.ui.chat.timeline.chip.as_ref()?;
    let Route::Thread(thread) = app.frame_route() else { return None };
    let view = app.state().thread_views.get(thread)?;
    let message = super::rows::array(&view.projection, "turnItems").iter().find(|i| s(i, "id") == item)?;
    let record = super::rows::array(&message["context"], "records").iter().find(|r| s(r, "contextId") == id)?;
    let screenshot = super::rows::array(message, "attachments")
        .iter()
        .find(|a| s(a, "name") == format!("preview-annotation-{}.png", s(record, "annotationId")))
        .map(|a| super::attachments::view(app, a, t));
    Some(card(t, record, screenshot).anchored(bubble_key(item), Side::Bottom, Align::End, 4.0).modal(true, |app: &mut App| {
        app.ui.chat.timeline.chip = None;
    }))
}

/// The details card for one record; `screenshot` is a picked element's crop when it is at hand.
pub fn card(t: &T, record: &Value, screenshot: Option<El>) -> El {
    let muted = t.c.muted_foreground;
    let head = |glyph: &'static rune::Icon, title: String, sub: String| {
        row()
            .w_full()
            .gap(6.0)
            .center_y()
            .child(icon(glyph, 14.0).color(muted))
            .child(text(title).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink())
            .child(text(sub).font(theme::TEXT_XS).color(muted).truncate().shrink())
    };
    let code = |body: &str, max_h: f32| {
        col()
            .w_full()
            .max_h(max_h)
            .padding(8.0)
            .radius(radius::MD)
            .fill(t.c.foreground.alpha_mul(0.04))
            .scroll(ScrollSpec { stick_bottom: false })
            .child(text(body.to_owned()).font(theme::MONO_XS).color(t.c.foreground).selectable(true))
    };
    let line = |body: String| text(body).font(theme::TEXT_XS).color(t.c.foreground).selectable(true);
    let mut body = col().w_full().gap(8.0);
    let pr = &record["pullRequest"];
    match s(record, "kind") {
        // PullRequestContextDetails.
        "review-comment" if pr.is_object() => {
            let (tone, glyph) = chips::look("review-comment", Some(record)).unwrap_or((muted, &icons::GIT_PULL_REQUEST_ARROW));
            let state = match (s(pr, "state"), pr["isDraft"] == true) {
                ("merged", _) => "Merged",
                ("closed", _) => "Closed",
                (_, true) => "Draft",
                _ => "Open",
            };
            body = body
                .gap(4.0)
                .child(
                    row()
                        .gap(6.0)
                        .center_y()
                        .child(icon(glyph, 14.0).color(tone))
                        .child(text(format!("Pull request #{}", pr["number"])).font(theme::XS_MEDIUM).color(t.c.foreground))
                        .child(text(state).font(theme::XS_MEDIUM).color(tone)),
                )
                .child(line(s(pr, "title").to_owned()))
                .child(
                    row()
                        .gap(4.0)
                        .center_y()
                        .child(text(s(pr, "headBranch").to_owned()).font(theme::MONO_XS).color(t.c.secondary_label).truncate().shrink())
                        .child(icon(&icons::ARROW_RIGHT, 12.0).color(t.c.secondary_label))
                        .child(text(s(pr, "baseBranch").to_owned()).font(theme::MONO_XS).color(t.c.secondary_label).truncate().shrink()),
                );
        }
        "terminal" => {
            let (a, b) = (record["lineStart"].as_u64().unwrap_or(0), record["lineEnd"].as_u64().unwrap_or(0));
            let lines = if a == b { format!("Line {a}") } else { format!("Lines {a}–{b}") };
            body = body.child(head(&icons::TERMINAL, s(record, "terminalLabel").to_owned(), lines)).child(code(s(record, "text"), 320.0));
        }
        "element" => {
            body = body.child(page_head(t, record)).children(selector(t, record)).child(code(s(record, "htmlPreview"), 160.0));
            if !s(record, "styles").is_empty() {
                body = body.child(code(s(record, "styles"), 128.0));
            }
        }
        "preview-annotation" => {
            body = body.children(screenshot).child(page_head(t, record));
            if !s(record, "comment").is_empty() {
                body = body.child(line(s(record, "comment").to_owned()));
            }
            if !s(record, "targetSummary").is_empty() {
                body = body.child(text(s(record, "targetSummary").to_owned()).font(theme::TEXT_XS).color(muted));
            }
            for element in super::rows::array(record, "elements") {
                body = body.children(selector(t, element)).child(code(s(element, "htmlPreview"), 96.0));
            }
        }
        _ => {
            let range = [s(record, "sectionTitle"), s(record, "rangeLabel")].into_iter().filter(|p| !p.is_empty()).collect::<Vec<_>>().join(" · ");
            body = body.child(head(&icons::MESSAGE_CIRCLE, s(record, "filePath").to_owned(), range));
            if !s(record, "text").is_empty() {
                body = body.child(line(s(record, "text").to_owned()));
            }
            if !s(record, "diff").is_empty() {
                body = body.child(code(s(record, "diff"), 256.0));
            }
        }
    }
    col()
        .key("context-chip-details")
        .w(384.0)
        .padding(12.0)
        .radius(radius::LG)
        .fill(t.c.popover)
        .border(1.0, t.c.border)
        .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
        .child(body)
}

fn page_head(t: &T, record: &Value) -> El {
    let url = s(record, "pageUrl");
    let title = record["pageTitle"].as_str().filter(|p| !p.trim().is_empty()).unwrap_or(url);
    col()
        .w_full()
        .gap(2.0)
        .child(text(title.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate())
        .child(text(url.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate())
}

fn selector(t: &T, element: &Value) -> Option<El> {
    let selector = element["selector"].as_str().filter(|v| !v.is_empty())?;
    let source = &element["source"];
    let place = match (source["file"].as_str(), source["line"].as_u64()) {
        (Some(file), Some(line)) => format!("{file}:{line}"),
        (Some(file), None) => file.to_owned(),
        _ => String::new(),
    };
    Some(
        row()
            .w_full()
            .gap(8.0)
            .child(text(selector.to_owned()).font(theme::MONO_XS).color(t.c.foreground).truncate().shrink())
            .child(text(place).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().shrink()),
    )
}
