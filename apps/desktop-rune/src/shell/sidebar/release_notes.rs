//! The app pill's release notes: Electron's `normalizeDesktopUpdateReleaseNotes` over the
//! releases between this build and the offered one, and web's `SidebarUpdateReleaseNotes`.

use rune::{Cursor, El, ScrollSpec, Span, Tx, col, icon, rich, row, text};
use serde_json::Value;

use crate::desktop::updates::{self, Status};
use crate::icons as i;
use crate::theme::{self, T};
use crate::{App, kit};

const MAX_GROUPS: usize = 6;
const MAX_ITEMS: usize = 8;
const MAX_ITEM_LEN: usize = 220;

/// `DesktopUpdateReleaseNote`.
#[derive(Debug, Default, PartialEq)]
pub struct Note {
    pub version: String,
    pub items: Vec<String>,
    pub total_items: usize,
    pub intro: Option<String>,
    pub stats: Option<String>,
    pub footer: Option<String>,
    pub released_at: Option<String>,
}

fn decode_entity(entity: &str) -> Option<String> {
    let code = |digits: &str, radix| u32::from_str_radix(digits, radix).ok().and_then(char::from_u32).map(String::from);
    match entity {
        "amp" => Some("&".into()),
        "apos" => Some("'".into()),
        "gt" => Some(">".into()),
        "lt" => Some("<".into()),
        "nbsp" => Some(" ".into()),
        "quot" => Some("\"".into()),
        _ => match entity.strip_prefix("#x").or(entity.strip_prefix("#X")) {
            Some(hex) if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) => code(hex, 16),
            _ => entity.strip_prefix('#').filter(|d| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit())).and_then(|d| code(d, 10)),
        },
    }
}

/// `decodeHtmlEntities`: named and numeric entities; anything else stays as written.
fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let decoded = tail.find(';').filter(|end| *end > 0 && *end <= 10).and_then(|end| decode_entity(&tail[..end]).map(|d| (d, end)));
        match decoded {
            Some((text, end)) => {
                out.push_str(&text);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// What a tag becomes in `stripMarkup`'s Markdown.
fn tag_text(tag: &str) -> String {
    let closing = tag.starts_with('/');
    let name: String = tag.trim_start_matches('/').chars().take_while(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let heading = name.len() == 2 && name.starts_with('h') && matches!(name.as_bytes()[1], b'1'..=b'6');
    match name.as_str() {
        "strong" | "b" => "**".into(),
        "br" if !closing => "\n".into(),
        "li" if !closing => "\n- ".into(),
        _ if heading && !closing => format!("\n{} ", "#".repeat((name.as_bytes()[1] - b'0') as usize)),
        "p" | "div" | "li" | "ul" | "ol" | "blockquote" if closing => "\n".into(),
        _ if heading && closing => "\n".into(),
        _ => String::new(),
    }
}

/// `\[([^\]]+)\]\([^)]+\)` → its text.
fn unlink(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        let link = tail.find(']').filter(|close| *close > 0).and_then(|close| {
            let url = tail[close + 1..].strip_prefix('(')?;
            let end = url.find(')').filter(|e| *e > 0)?;
            Some((&tail[..close], close + 2 + end + 1))
        });
        match link {
            Some((label, used)) => {
                out.push_str(label);
                rest = &tail[used..];
            }
            None => {
                out.push('[');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `\*\*([^*]+)\*\*` → its text.
fn unbold(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find("**") {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 2..];
        match tail.find('*').filter(|end| *end > 0 && tail[*end..].starts_with("**")) {
            Some(end) => {
                out.push_str(&tail[..end]);
                rest = &tail[end + 2..];
            }
            None => {
                out.push_str("**");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `stripMarkup`: release HTML to plain lines, list items as "- ", headings as "#".
fn strip_markup(input: &str, keep_bold: bool) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(open) = rest.find('<') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        match tail.find('>') {
            Some(close) => {
                out.push_str(&tag_text(&tail[..close]));
                rest = &tail[close + 1..];
            }
            None => {
                out.push('<');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    let markdown = decode_entities(&unlink(&out));
    if keep_bold { markdown } else { unbold(&markdown) }
}

fn collapse(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_heading(line: &str) -> bool {
    let hashes = line.len() - line.trim_start_matches('#').len();
    (1..=6).contains(&hashes) && line[hashes..].starts_with(char::is_whitespace)
}

fn normalize_line(line: &str) -> String {
    line.to_lowercase().chars().filter(|c| !matches!(c, '*' | '_' | '`' | '#')).collect::<String>().trim().to_owned()
}

fn is_ignored(line: &str) -> bool {
    let normalized = normalize_line(line);
    let range = normalized.strip_prefix("full changelog: ").is_some_and(|r| !r.contains(char::is_whitespace) && r.split_once("...").is_some_and(|(a, b)| !a.is_empty() && !b.is_empty()));
    normalized.is_empty() || normalized == "what's changed" || normalized == "whats changed" || normalized.starts_with("compare: ") || normalized.contains("/compare/") || range
}

fn truncate(item: String) -> String {
    if item.chars().count() <= MAX_ITEM_LEN {
        return item;
    }
    format!("{}...", item.chars().take(MAX_ITEM_LEN - 3).collect::<String>().trim_end())
}

/// Drops a trailing ` (#12, #34).` PR reference list.
fn without_refs(line: &str) -> &str {
    let trimmed = line.strip_suffix('.').unwrap_or(line);
    let Some(inner) = trimmed.strip_suffix(')').and_then(|t| t.rfind('(').map(|open| (open, &t[open + 1..]))) else { return line };
    let (open, refs) = inner;
    let valid = !refs.is_empty() && refs.split(',').all(|r| r.trim().strip_prefix('#').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())));
    if valid { trimmed[..open].trim_end() } else { line }
}

/// `extractReleaseNoteSummary`: a generated summary's stats line, intro, highlights and footer.
fn extract_summary(summary: &str) -> Note {
    let (mut note, mut intro) = (Note::default(), Vec::new());
    for raw in strip_markup(summary, true).split('\n') {
        let line = without_refs(&collapse(raw)).trim().to_owned();
        let plain = unbold(&line);
        if let Some(rest) = plain.strip_prefix("### Plus ")
            && rest.split_once(' ').is_some_and(|(n, more)| n.bytes().all(|b| b.is_ascii_digit()) && !n.is_empty() && more.starts_with("more "))
        {
            note.footer = Some(plain[4..].to_owned());
            continue;
        }
        if plain.is_empty() || is_heading(&plain) {
            continue;
        }
        if note.stats.is_none()
            && plain.starts_with('v')
            && let Some((version, stats)) = plain.split_once(" · ")
            && !version.contains(char::is_whitespace)
            && version.len() > 1
        {
            note.stats = Some(stats.to_owned());
            continue;
        }
        match line.strip_prefix("- ").or(line.strip_prefix("* ")) {
            Some(item) => note.items.push(item.trim_start().to_owned()),
            None => intro.push(plain),
        }
    }
    note.total_items = note.items.len() + intro.len();
    note.intro = (!intro.is_empty()).then(|| intro.join(" "));
    note
}

/// Where a generated summary folds GitHub's full list (`<details><summary>Full changelog</summary>`).
fn summary_end(note: &str) -> Option<usize> {
    let lower = note.to_ascii_lowercase();
    lower.match_indices("<details>").map(|(at, _)| at).find(|at| {
        let rest = lower[at + "<details>".len()..].trim_start();
        rest.strip_prefix("<summary>").map(str::trim_start).and_then(|r| r.strip_prefix("full changelog")).map(str::trim_start).is_some_and(|r| r.starts_with("</summary>"))
    })
}

/// `extractReleaseNoteItems`.
fn extract(note: &str) -> Note {
    if note.is_empty() {
        return Note::default();
    }
    if let Some(end) = summary_end(note) {
        return extract_summary(&note[..end]);
    }
    let mut items: Vec<String> = Vec::new();
    let mut total = 0;
    for raw in strip_markup(note, false).split('\n') {
        let mut item = raw.trim();
        item = item.strip_prefix("- ").or(item.strip_prefix("* ")).map_or(item, str::trim_start);
        let digits = item.len() - item.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits > 0 && matches!(item.as_bytes().get(digits), Some(b'.' | b')')) && item[digits + 1..].starts_with(char::is_whitespace) {
            item = item[digits + 1..].trim_start();
        }
        let item = collapse(item);
        let normalized = normalize_line(&item);
        if normalized == "new contributors" || normalized == "full changelog" {
            break;
        }
        if is_heading(&item) || is_ignored(&item) {
            continue;
        }
        total += 1;
        items.push(truncate(item));
        if items.len() > MAX_ITEMS {
            items.remove(0);
        }
    }
    items.reverse();
    Note { items, total_items: total, ..Note::default() }
}

/// `resolveReleasedAt`: the offered release's publish time; a prerelease's UTC build day.
fn released_at(version: &str, offered: &str, published: Option<&str>) -> Option<String> {
    if version == offered && let Some(at) = published {
        return Some(at.to_owned());
    }
    let (_, build) = version.split_once("-nightly.").or_else(|| version.split_once("-preview."))?;
    let (day, count) = build.split_once('.')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    (day.len() == 8 && digits(day) && digits(count)).then(|| format!("{}-{}-{}", &day[..4], &day[4..6], &day[6..]))
}

/// The notes to show and how many older releases didn't fit, from `updates`' newest-first history.
pub fn normalize(history: &[Value], offered: &str, fallback: &str) -> (Vec<Note>, usize) {
    let notes: Vec<Note> = history
        .iter()
        .filter_map(|entry| {
            let version = entry["version"].as_str()?;
            let body = entry["note"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or(if version == offered { fallback } else { "" });
            let note = extract(body);
            if note.total_items == 0 && note.stats.is_none() {
                return None;
            }
            Some(Note { version: version.to_owned(), released_at: released_at(version, offered, entry["publishedAt"].as_str()), ..note })
        })
        .collect();
    let omitted = notes.len().saturating_sub(MAX_GROUPS);
    (notes.into_iter().take(MAX_GROUPS).collect(), omitted)
}

/// `formatReleaseAge`: feed times read "3h ago"; build days read "today" or "2d ago".
fn age(released_at: &str, now: i64) -> String {
    let day = |s: &str| -> Option<i64> {
        let (y, rest) = s.split_once('-')?;
        let (m, d) = rest.split_once('-')?;
        (y.len() == 4 && m.len() == 2 && d.len() == 2).then_some(())?;
        crate::data::iso_ms(&format!("{y}-{m}-{d}T00:00:00.000Z"))
    };
    match day(released_at) {
        Some(at) => {
            let days = (now.div_euclid(86_400_000) * 86_400_000 - at) / 86_400_000;
            if days <= 0 { "today".to_owned() } else { format!("{days}d ago") }
        }
        None => crate::data::iso_ms(released_at).map_or_else(String::new, |at| crate::shell::palette::relative_time(now, at)),
    }
}

/// The notes this update would show; empty when there are none (`releaseNotes.length === 0`).
pub fn current(s: &App) -> (Vec<Note>, usize) {
    let ui = &s.ui.desktop.updates;
    let Some(release) = ui.release.as_ref() else { return (Vec::new(), 0) };
    let history: Vec<Value> = if ui.history.is_empty() { vec![serde_json::json!({"version": release.version})] } else { ui.history.clone() };
    normalize(&history, &release.version, &release.notes)
}

fn link(label: String, url: String, t: &T) -> El {
    row()
        .mt(4.0)
        .gap(4.0)
        .center_y()
        .role("link")
        .cursor(Cursor::Pointer)
        .child(text(label).font(theme::TEXT_XS).color(t.c.muted_foreground))
        .child(icon(&i::EXTERNAL_LINK, 12.0).color(t.c.muted_foreground))
        .on_press_tx(move |_: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(&url);
        })
}

fn bullet(item: &str, t: &T) -> El {
    // A "**Feature** what it does" highlight sets its name in bold.
    let highlight = item.strip_prefix("**").and_then(|rest| rest.split_once("** ")).filter(|(name, _)| !name.is_empty() && !name.contains('*'));
    let body = match highlight {
        Some((name, rest)) => {
            let line = format!("{name} {rest}");
            rich(line, vec![Span::new(0, name.len()).weight(theme::SEMIBOLD)])
        }
        None => text(item.to_owned()),
    };
    row().gap(6.0).child(text("•").font(theme::TEXT_XS).color(t.c.muted_foreground)).child(body.font(theme::TEXT_XS).color(t.c.foreground).wrap().grow(1.0).shrink())
}

/// `SidebarUpdateReleaseNotes`, anchored over the pill.
pub fn view(s: &App, t: &T, on_hover: fn(&mut App, bool), layers: &mut Vec<El>) {
    let Some(release) = s.ui.desktop.updates.release.as_ref() else { return };
    let (notes, omitted_releases) = current(s);
    if notes.is_empty() {
        return;
    }
    let now = crate::data::now_ms();
    let muted = t.c.muted_foreground;
    let offered = release.version.as_str();
    let head = if s.ui.desktop.updates.status == Status::Available {
        let age = notes.iter().find(|n| n.version == offered).and_then(|n| n.released_at.as_deref()).map(|at| age(at, now));
        let line = match age {
            Some(age) if !age.is_empty() => format!("{offered} · {age}"),
            _ => offered.to_owned(),
        };
        col().px(4.0).child(text("Update ready to download").font(theme::SM_MEDIUM)).child(text(line).font(theme::TEXT_XS).color(muted).mt(2.0))
    } else {
        col().px(4.0).child(text(updates::summary(s)).font(theme::SM_MEDIUM).wrap())
    };
    let mut list = col().px(4.0).pt(12.0).pb(4.0);
    for (index, note) in notes.iter().enumerate() {
        let mut section = col().gap(6.0).mt(if index > 0 { 16.0 } else { 0.0 });
        let generated = note.stats.is_some() || note.intro.is_some();
        if generated {
            if note.version != offered {
                let label = match note.released_at.as_deref().map(|at| age(at, now)).filter(|a| !a.is_empty()) {
                    Some(age) => format!("{} · {age}", note.version),
                    None => note.version.clone(),
                };
                section = section.child(text(label).font(theme::XS_MEDIUM).color(muted));
            }
        } else {
            let label = if index == 0 { "What's changed".to_owned() } else { format!("Changes in {}", note.version) };
            section = section.child(text(label).font(theme::XS_MEDIUM).color(t.c.foreground));
        }
        if let Some(intro) = &note.intro {
            section = section.child(text(intro.clone()).font(theme::TEXT_XS).color(t.c.foreground).wrap());
        } else {
            for item in &note.items {
                section = section.child(bullet(item, t));
            }
        }
        if let Some(footer) = &note.footer {
            section = section.child(text(footer.clone()).font(theme::XS_MEDIUM).wrap());
        }
        let omitted = note.total_items.saturating_sub(note.items.len());
        let label = note.stats.clone().unwrap_or_else(|| match omitted {
            0 => "View release on GitHub".to_owned(),
            n => format!("{n} more {} on GitHub", if n == 1 { "change" } else { "changes" }),
        });
        list = list.child(section.child(link(label, updates::release_page(&note.version), t)));
    }
    if omitted_releases > 0 {
        let n = omitted_releases;
        list = list.child(col().mt(12.0).child(link(format!("{n} older {} on GitHub", if n == 1 { "release" } else { "releases" }), updates::releases_page(), t)));
    }
    let k = kit::theme(t);
    // A fixed width (web's 24rem cap, which notes reach) so wrapped lines measure as they draw.
    let body = col().w(384.0).padding(8.0).role("dialog").label("Update release notes").child(head).child(kit::scroll_area("sb-release-notes", list.max_h(448.0).scroll(ScrollSpec { stick_bottom: false }), t));
    let surface = kit::menu::glass(body, &k, false)
        .hover_delays(std::time::Duration::ZERO, std::time::Duration::from_millis(150))
        .on_hover_change(move |s: &mut App, on, _: &mut Tx| on_hover(s, on));
    layers.push(kit::motion::popup(rune_kit::popover::anchored(&k, surface, "sb-app-update", rune_kit::Placement::Top, 4.0)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_notes_list_their_latest_items_and_skip_github_boilerplate() {
        let body = "## What's Changed\n* Fix the sidebar by @a in https://x/pull/1\n* **Speech** menu &amp; voice (#12, #13).\n\n**Full Changelog**: https://github.com/o/r/compare/v1...v2\n## New Contributors\n* @b made a first contribution";
        let note = extract(body);
        assert_eq!(note.items, ["Speech menu & voice (#12, #13).", "Fix the sidebar by @a in https://x/pull/1"]);
        assert_eq!(note.total_items, 2);
        let many: String = (1..=10).map(|n| format!("- item {n}\n")).collect();
        let note = extract(&many);
        assert_eq!((note.total_items, note.items.len(), note.items[0].as_str()), (10, MAX_ITEMS, "item 10"), "FAILED: keeps the last eight, newest first");
        assert_eq!(extract("<ul><li>One<br/>Two</li></ul>").items, ["Two", "One"]);
    }

    #[test]
    fn generated_summaries_keep_stats_intro_highlights_and_footer() {
        let body = "<p>v0.1.3 · 12 PRs · 4 contributors</p>\n<ul><li><strong>Speech</strong> menu for dictation (#40)</li><li>Faster streaming</li></ul>\n<h3>Plus 5 more fixes</h3>\n<details><summary>Full changelog</summary>* everything</details>";
        let note = extract(body);
        assert_eq!(note.stats.as_deref(), Some("12 PRs · 4 contributors"));
        assert_eq!(note.items, ["**Speech** menu for dictation", "Faster streaming"]);
        assert_eq!(note.footer.as_deref(), Some("Plus 5 more fixes"));
        assert_eq!(extract("Faster device streaming.\n<details> <summary>Full changelog</summary></details>").intro.as_deref(), Some("Faster device streaming."));
    }

    #[test]
    fn history_shows_six_releases_and_counts_the_older_ones() {
        let history: Vec<Value> = (0..8).rev().map(|n| json!({"version": format!("0.1.{n}"), "note": format!("- change {n}"), "publishedAt": "2026-11-20T10:00:00Z"})).collect();
        let (notes, omitted) = normalize(&history, "0.1.7", "");
        assert_eq!((notes.len(), omitted), (MAX_GROUPS, 2));
        assert_eq!(notes[0].released_at.as_deref(), Some("2026-11-20T10:00:00Z"), "FAILED: only the offered release is dated by the feed");
        assert_eq!(notes[1].released_at, None);
        let (notes, _) = normalize(&[json!({"version": "0.1.3"})], "0.1.3", "- From the manifest");
        assert_eq!(notes[0].items, ["From the manifest"], "FAILED: the manifest's notes stand in for an empty release body");
        assert_eq!(released_at("0.1.4-nightly.20261122.4", "0.1.5", None).as_deref(), Some("2026-11-22"));
        let now = crate::data::iso_ms("2026-11-24T15:00:00.000Z").expect("time");
        assert_eq!((age("2026-11-22", now), age("2026-11-24", now)), ("2d ago".to_owned(), "today".to_owned()));

        let release = |tag: &str, prerelease: bool| json!({"tag_name": tag, "prerelease": prerelease, "draft": false, "body": tag});
        let releases = json!([release("rune-v0.1.1", false), release("rune-v0.1.4-nightly.20261122.1", true), release("rune-v0.1.3", false), release("rune-v0.1.2", false), release("other-v9", false)]);
        let versions = |found: Vec<Value>| found.iter().map(|r| r["version"].as_str().unwrap_or_default().to_owned()).collect::<Vec<_>>();
        assert_eq!(versions(updates::history(&releases, "latest", "0.1.1", "0.1.3")), ["0.1.3", "0.1.2"], "FAILED: newer than this build, up to the offer, newest first");
        assert_eq!(versions(updates::history(&releases, "nightly", "0.1.1", "0.1.4-nightly.20261122.1")), ["0.1.4-nightly.20261122.1", "0.1.3", "0.1.2"]);
    }
}
