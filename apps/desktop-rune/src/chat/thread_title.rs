//! deriveThreadTitleSeed (client-runtime operations/threadTitle.ts): a new thread's title until
//! the server generates one, and the `titleSeed` every send carries.
use std::collections::HashSet;

use serde_json::Value;

use crate::chat::composer::{attachments, chips};
use crate::chat::timeline::citation;
use crate::data::s;

/// The seed for an outgoing message: its text, else its attached files (paths), else the first
/// terminal, review comment or picked element among its context records (sendTitleFallbackLabels).
pub fn seed(text: &str, paths: &[String], contexts: &[Value]) -> String {
    let prose = prompt_text(text);
    if !prose.is_empty() {
        return truncate(&prose);
    }
    let names: Vec<String> = paths.iter().map(|p| attachments::file_name(p)).collect();
    match names.iter().filter(|n| attachments::is_image(n)).count() {
        0 => {}
        1 => return "Attached image".into(),
        _ => return "Attached images".into(),
    }
    let name = normalize(names.first().map_or("", String::as_str));
    if !name.is_empty() {
        return truncate(&format!("File: {name}"));
    }
    let first = |kind: &str| contexts.iter().find(|r| s(r, "kind") == kind).map(|r| s(r, "label"));
    let fallbacks = [
        first("terminal").map(str::to_owned),
        first("review-comment").map(|l| format!("Review: {l}")),
        first("preview-annotation").map(str::to_owned),
    ];
    fallbacks.iter().flatten().map(|l| normalize(l)).find(|l| !l.is_empty()).map_or_else(|| "New thread".into(), |l| truncate(&l))
}

/// threadTitleTextFromPrompt: context references read as their labels, quotes as their text,
/// and a prompt of only images as "Attached image(s)".
fn prompt_text(text: &str) -> String {
    let (shown, chips) = chips::editable(text);
    let (mut prose, mut images, mut at) = (String::new(), HashSet::new(), 0);
    for chip in &chips {
        prose.push_str(&shown[at..chip.range.start]);
        at = chip.range.end;
        match chip.kind.as_str() {
            "image" => {
                images.insert(chip.id.as_str());
            }
            // assistantCitationsToPlainText
            "citation" => match citation::parse_href(&chip.id) {
                Some(quote) => {
                    prose.push_str(&quote.text);
                    if let Some(comment) = &quote.comment {
                        prose.push_str("\nComment: ");
                        prose.push_str(comment);
                    }
                }
                None => prose.push_str(&shown[chip.range.clone()]),
            },
            // A file mention is not a context reference: web keeps its link text.
            "mention" => prose.push_str(&chips::file_link(&chip.id)),
            _ => prose.push_str(&shown[chip.range.clone()]),
        }
    }
    prose.push_str(&shown[at..]);
    let prose = normalize(&prose);
    match images.len() {
        _ if !prose.is_empty() => prose,
        0 => String::new(),
        1 => "Attached image".into(),
        _ => "Attached images".into(),
    }
}

fn normalize(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// truncate (shared String.ts): 50 characters, then "...".
fn truncate(text: &str) -> String {
    let text = text.trim();
    match text.char_indices().nth(50) {
        Some((at, _)) => format!("{}...", &text[..at]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn titles_seed_from_the_prompt_then_attachments_then_context_labels() {
        assert_eq!(seed("  showcase \n please ", &[], &[]), "showcase please");
        assert_eq!(seed(&"x".repeat(51), &[], &[]), format!("{}...", "x".repeat(50)));
        assert_eq!(seed("Why does [Terminal 1 lines 3-4](t3-context://v1/terminal/terminal_a) fail?", &[], &[]), "Why does Terminal 1 lines 3-4 fail?");
        assert_eq!(seed("![shot.png](t3-context://v1/image/image_a)", &[], &[]), "Attached image");
        assert_eq!(seed("![a.png](t3-context://v1/image/image_a) ![b.png](t3-context://v1/image/image_b)", &[], &[]), "Attached images");
        assert_eq!(seed("", &["/tmp/a.png".into(), "/tmp/b.jpg".into()], &[]), "Attached images");
        assert_eq!(seed("", &["/tmp/build.log".into()], &[]), "File: build.log");
        let contexts = [json!({"kind": "review-comment", "label": "main.rs L4"}), json!({"kind": "terminal", "label": "Terminal 1 lines 1-9"})];
        assert_eq!(seed("", &[], &contexts), "Terminal 1 lines 1-9");
        assert_eq!(seed("", &[], &contexts[..1]), "Review: main.rs L4");
        assert_eq!(seed("", &[], &[]), "New thread");
    }
}
