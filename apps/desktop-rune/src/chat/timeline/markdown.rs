use super::find::Marks;
use std::borrow::Cow;
use super::{copy_button, language_icons, md_links, media};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, icon, row, text};
use rune_kit::markdown::{Markdown, MarkdownStyle};

/// Reply text: foreground at 90%.
fn ink(t: &T) -> rune::Color {
    t.c.foreground.alpha_mul(0.9)
}

fn style(t: &T, ink: rune::Color) -> MarkdownStyle {
    MarkdownStyle::neutral(t.dark)
        .text(ink)
        .muted(t.c.muted_foreground)
        .link(t.c.info_foreground)
        .body_font(theme::TEXT_SM.lh(22.75))
        .heading_font(1, theme::weight(theme::TEXT_XL.lh(26.0), 600.0))
        .heading_font(2, theme::weight(theme::TEXT_LG.lh(23.4), 600.0))
        .heading_font(3, theme::weight(theme::TEXT_BASE.lh(20.8), 600.0))
        .inline_code_bg(t.c.foreground.alpha_mul(0.1))
        .inline_code_font(0.85, 400.0)
        .inline_code_pad(4.0)
        .strong_weight(600.0)
        .list(20.0, 4.0)
        .code_block(code_background(t), rune::Color::TRANSPARENT)
        // Tuned at 12/19.25 for the default 13px code size; both follow the setting.
        .code_block_font(theme::mono(rune::TextStyle::new(12.0 * t.code / 13.0, 400.0).lh(19.25 * t.code / 13.0)))
        .code_block_padding(14.4, 12.8)
        .table_outline(None)
        .table_rules(t.c.border.alpha_mul(0.6))
        .table_header_fill(rune::Color::TRANSPARENT)
        .table_header_text(t.c.foreground)
        .table_header_weight(600.0)
        .table_full_width(true)
        .table_cell_padding(12.0, 7.2)
        .table_header_pad(12.0, 8.8)
        .table_flush_left(true)
        .table_font(theme::TEXT_SM.lh(22.75))
        .table_tabular_figures(false)
        .rule(t.c.border)
        .quote_rule(t.c.foreground.alpha_mul(0.35))
}

fn code_background(t: &T) -> rune::Color {
    if t.dark {
        t.c.input.alpha_mul(0.32)
    } else {
        t.c.secondary
    }
}

fn marks_for(marks: Option<&Marks>, src: &str) -> Vec<(Vec<std::ops::Range<usize>>, rune_kit::markdown::Highlight)> {
    marks.map_or_else(Vec::new, |m| m.markdown(src))
}

/// `key` names this run of prose within its message, for the blocks' own keys.
fn prose(app: &App, source: &str, key: &str, t: &T, marks: Option<&Marks>, ink: rune::Color) -> El {
    // Find counts matches in the TeX source, so a marked message keeps it.
    if t.math && marks.is_none() && super::math::may_contain(source) {
        return typeset(app, source, key, t, ink);
    }
    let source = if t.math { super::math::literal(source) } else { Cow::Borrowed(source) };
    let source: &str = &source;
    let mut out = col().w_full().gap(10.4);
    let parents = md_links::parent_suffixes(source);
    for (index, block) in source
        .trim()
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
        .enumerate()
    {
        let mut block_style = style(t, ink);
        if block.starts_with("####") {
            block_style =
                block_style.heading_font(3, theme::weight(theme::TEXT_SM.lh(18.2), 600.0));
        }
        let is_table = block
            .lines()
            .nth(1)
            .is_some_and(|line| line.contains("---") && line.contains('|'));
        // Find counts matches in rendered Markdown, so a marked block keeps rune_kit's rendering.
        let own = !is_table && marks.is_none();
        let pictures = if own { media::block(app, block, t).or_else(|| media::inline(app, block, theme::TEXT_SM.lh(22.75), ink, t)) } else { None };
        let chips = if own && pictures.is_none() { md_links::inline_block(app, block, format!("md-block-{key}-{index}"), t, ink, theme::TEXT_SM.lh(22.75), &parents) } else { None };
        let content = if let Some(pictures) = pictures {
            pictures
        } else if let Some(chips) = chips {
            chips
        } else if is_table {
            table(app, block, t, marks, ink)
        } else {
            let content = Markdown::new(block)
                .selectable(true)
                .highlights(marks_for(marks, block))
                .on_link_tx(md_links::open)
                .link_style(md_links::look(t))
                .link_help(md_links::help)
                .render(&block_style);
            if index > 0 && block.starts_with('#') { col().pt(9.6).child(content) } else { content }
        };
        out = out.child(content);
    }
    out
}

/// Prose holding math: display formulas, and paragraphs of inline ones, as KaTeX pictures once
/// typeset; everything else, and math still typesetting, as Markdown with literal TeX.
fn typeset(app: &App, source: &str, key: &str, t: &T, ink: rune::Color) -> El {
    use super::math::Block;
    let plain = T { math: false, ..*t };
    let font = theme::TEXT_SM.lh(22.75);
    let mut out = col().w_full().gap(10.4);
    let mut pending = String::new();
    let mut runs = 0;
    let mut flush = |out: El, pending: &mut String| {
        if pending.is_empty() {
            return out;
        }
        runs += 1;
        out.child(prose(app, &std::mem::take(pending), &format!("{key}-m{runs}"), &plain, None, ink))
    };
    for block in super::math::blocks(source.trim()) {
        let picture = match &block {
            Block::Display(tex) => display_math(app, tex, ink),
            Block::Inline(parts) => super::katex::flow(app, parts, font, ink, t),
            Block::Text(_) => None,
        };
        match picture {
            Some(picture) => out = flush(out, &mut pending).child(picture),
            None => {
                if !pending.is_empty() {
                    pending.push_str("\n\n");
                }
                pending.push_str(&block.literal());
            }
        }
    }
    flush(out, &mut pending)
}

/// A display formula's picture, centred as `.katex-display` is.
fn display_math(app: &App, tex: &str, ink: rune::Color) -> Option<El> {
    super::katex::picture(app, tex, true, theme::TEXT_SM.size, ink).map(|picture| row().w_full().center().child(picture))
}

fn table(app: &App, source: &str, t: &T, marks: Option<&Marks>, ink: rune::Color) -> El {
    if source.contains("\\|") || source.lines().nth(1).is_some_and(|line| line.contains(':')) {
        return Markdown::new(source)
            .selectable(true)
            .highlights(marks_for(marks, source))
            .on_link_tx(md_links::open)
            .link_style(md_links::look(t))
            .link_help(md_links::help)
            .render(&style(t, ink));
    }
    let lines: Vec<Vec<&str>> = source
        .lines()
        .enumerate()
        .filter(|(i, _)| *i != 1)
        .map(|(_, line)| {
            line.trim()
                .trim_matches('|')
                .split('|')
                .map(str::trim)
                .collect()
        })
        .collect();
    let columns = lines.first().map_or(0, Vec::len);
    let mut widths = vec![0.0_f32; columns];
    for (i, cells) in lines.iter().enumerate() {
        for (j, cell) in cells.iter().take(columns).enumerate() {
            let code = cell.starts_with('`') && cell.ends_with('`');
            let font = if code {
                theme::mono(rune::tok::TextStyle {
                    size: 11.9,
                    ..theme::TEXT_SM
                })
            } else if i == 0 {
                theme::weight(theme::TEXT_SM, 600.0)
            } else {
                theme::TEXT_SM
            };
            let measured = rune::text::measure(cell.trim_matches('`'), &[], font, None).0;
            widths[j] = widths[j].max(
                measured
                    + if code { 10.33 } else { 0.0 }
                    + if j == 0 || j + 1 == columns {
                        12.0
                    } else {
                        24.0
                    },
            );
        }
    }
    let total: f32 = widths.iter().sum();
    let mut table = col().w_full();
    for (i, cells) in lines.iter().enumerate() {
        let mut line = row().w_full();
        for (j, cell) in cells.iter().take(columns).enumerate() {
            let font = if i == 0 {
                theme::weight(theme::TEXT_SM.lh(22.75), 600.0)
            } else {
                theme::TEXT_SM.lh(22.75)
            };
            line = line.child(
                col()
                    .w_frac(widths[j] / total.max(1.0))
                    .min_w(0.0)
                    .pl(if j == 0 { 0.0 } else { 12.0 })
                    .pr(if j + 1 == columns { 0.0 } else { 12.0 })
                    .py(if i == 0 { 8.8 } else { 7.2 })
                    .child(
                        Markdown::new(*cell)
                            .highlights(marks_for(marks, cell))
                            .on_link_tx(md_links::open)
                            .link_style(md_links::look(t))
                            .link_help(md_links::help)
                            .selectable(true)
                            .render(&style(t, ink).body_font(font)),
                    ),
            );
        }
        table = table
            .child(line)
            .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)));
    }
    col().w_full().child(table).child(
        row()
            .w_full()
            .justify_end()
            .pt(2.0)
            .child(copy_button(app, source, t, "Copy table")),
    )
}

/// Parsing and highlighting a message is the costliest part of a timeline build, and the view
/// rebuilds at least once a second (relative times), so each message builds once per version.
/// A Mermaid source drawn as chat draws a ```` ```mermaid ```` fence, or `None` outside the
/// diagrams Rune supports.
pub(crate) fn diagram(source: &str, t: &T) -> Option<El> {
    rune_kit::mermaid::render(source, &style(t, t.c.foreground))
}

pub(crate) fn diagram_overlay(app: &App, t: &T, overlays: &mut Vec<El>) {
    let Some(card) = app.ui.chat.timeline.expanded_diagram.as_deref().and_then(|source| diagram(source, t)) else { return };
    crate::kit::Dialog::new("expanded-diagram", "Diagram")
        .open(true)
        .width(960.0)
        .child(col().key("expanded-diagram-scroll").w_full().shrink().min_h(0.0).max_h(520.0)
            .scroll(rune::ScrollSpec { stick_bottom: false })
            .child(card))
        .on_close(|s: &mut App| s.ui.chat.timeline.expanded_diagram = None)
        .build(t, overlays);
}

pub fn view(app: &App, source: &str, id: &str, t: &T) -> El {
    memoized(app, source, id, t, ink(t), false)
}

fn memoized(app: &App, source: &str, id: &str, t: &T, ink: rune::Color, run: bool) -> El {
    let ui = &app.ui.chat.timeline;
    let fence = format!("{id}-fence-");
    let mut unwrapped: Vec<&String> = ui.wrapped.iter().filter(|k| k.starts_with(&fence)).collect();
    unwrapped.sort();
    let details = format!("{id}-details-");
    let mut toggled: Vec<&String> = ui.expanded.iter().filter(|k| k.starts_with(&details)).collect();
    toggled.sort();
    let copied = ui.copied.as_ref().is_some_and(|(c, _)| source.contains(c.as_str()) && super::copied(app, c));
    let c = &t.c;
    let colors = format!(
        "{:?}",
        (c.foreground, c.background, c.muted_foreground, c.info_foreground, c.border, c.primary, c.secondary, c.input)
    );
    let version = (source, t.dark, colors, format!("{ink:?}"), unwrapped, toggled, copied, t.code.to_bits(), media::signature(app, source), run, t.math, super::katex::signature(app, source));
    rune::memo(format!("md-{id}"), version, || build(app, source, id, t, None, ink, run))
}

/// A reply, with find-in-thread matches painted (rows with marks build fresh while the bar is
/// open), its text a quote source for the Cite toolbar; once finished, its shell code blocks can
/// run in the thread's terminal.
pub fn reply(app: &App, source: &str, id: &str, t: &T, marks: Option<&Marks>, run: bool) -> El {
    let body = match marks {
        None => memoized(app, source, id, t, ink(t), run),
        Some(_) => build(app, source, id, t, marks, ink(t), run),
    };
    super::citation::source(id, body)
}

/// MarkdownCodeBlock `canRun`: a closed shell fence holding one visible command line. Control
/// and format characters (bidi overrides, zero-width) could make the shown command differ from
/// what the terminal receives.
fn runnable(language: &str, code: &str, closed: bool) -> bool {
    let format = |c: char| matches!(c, '\u{ad}' | '\u{600}'..='\u{605}' | '\u{61c}' | '\u{6dd}' | '\u{70f}' | '\u{180e}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' | '\u{e0001}' | '\u{e0020}'..='\u{e007f}');
    let command = code.trim();
    closed
        && matches!(language, "sh" | "bash" | "zsh" | "fish" | "shell" | "powershell" | "pwsh")
        && !command.is_empty()
        && !command.ends_with('\\')
        && !code.chars().any(|c| c.is_control() || format(c))
}

/// A user message in its bubble's ink, each newline a line break (ChatMarkdown `lineBreaks`).
pub fn user(app: &App, text: &str, id: &str, t: &T, marks: Option<&Marks>) -> El {
    let source = user_source(text);
    match marks {
        None => memoized(app, &source, id, t, t.c.message_foreground, false),
        Some(_) => build(app, &source, id, t, marks, t.c.message_foreground, false),
    }
}

/// `text` with every newline outside a code fence made a hard break; find counts matches in this.
pub fn user_source(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        let opens = ["```", "~~~"].into_iter().find(|m| line.trim_start().starts_with(*m));
        match (fence, opens) {
            (None, Some(marker)) => fence = Some(marker),
            (Some(open), Some(marker)) if open == marker => fence = None,
            (None, None) => {
                if let Some(body) = line.strip_suffix('\n').filter(|b| !b.trim().is_empty()) {
                    out.push_str(body.trim_end_matches('\r'));
                    out.push_str("  \n");
                    continue;
                }
            }
            _ => {}
        }
        out.push_str(line);
    }
    out
}

fn build(app: &App, source: &str, id: &str, t: &T, marks: Option<&Marks>, ink: rune::Color, run: bool) -> El {
    let out = render(&Cx { app, id, t, marks, ink, run }, source, "");
    if source.is_empty() {
        return out.child(icon(&icons::ELLIPSIS, 16.0).color(t.c.muted_foreground));
    }
    out
}

/// What every block of one message draws with.
struct Cx<'a> {
    app: &'a App,
    id: &'a str,
    t: &'a T,
    marks: Option<&'a Marks<'a>>,
    ink: rune::Color,
    /// A finished reply: its shell fences can run in the terminal.
    run: bool,
}

/// `source` as its blocks. `scope` tells a nested run's keys apart; the top level's is empty, so
/// its fences stay `{id}-fence-{n}`.
fn render(cx: &Cx, source: &str, scope: &str) -> El {
    let (app, id, t) = (cx.app, cx.id, cx.t);
    let mut out = col().w_full().gap(10.4);
    let (mut fences, mut details, mut nested) = (0, 0, 0);
    for part in parts(source) {
        let block = match part {
            Part::Prose { start, text } => prose(app, &text, &format!("{id}-{scope}{start}"), t, cx.marks, cx.ink),
            Part::Fence { language, code, closed } => {
                fences += 1;
                if t.math && closed && language == "math" {
                    // Already literal: `math: false` keeps prose from reading it again.
                    display_math(app, code.trim(), cx.ink).filter(|_| cx.marks.is_none()).unwrap_or_else(|| {
                        prose(app, &super::math::display(&code), &format!("{id}-{scope}math{fences}"), &T { math: false, ..*t }, cx.marks, cx.ink)
                    })
                } else {
                    fence(cx, &language, &code, closed, format!("{id}-fence-{scope}{}", fences - 1))
                }
            }
            Part::Alert { kind, body } => {
                nested += 1;
                alert(kind, render(cx, &body, &format!("{scope}a{nested}.")), t)
            }
            Part::Details { summary, open, body } => {
                details += 1;
                let key = format!("{id}-details-{scope}{}", details - 1);
                // `expanded` holds the blocks toggled away from how they were written.
                let open = open != app.ui.chat.timeline.expanded.contains(&key);
                nested += 1;
                let body = open.then(|| render(cx, &body, &format!("{scope}d{nested}.")));
                disclosure(&summary, key, open, body, t)
            }
            Part::Template(template) => template_card(&template, t),
        };
        out = out.child(block);
    }
    out
}

fn fence(cx: &Cx, language: &str, code: &str, closed: bool, key: String) -> El {
    let (app, t) = (cx.app, cx.t);
    // `wrapped` holds the blocks toggled away from the wordWrap setting.
    let wrap = (crate::settings::value(app, "wordWrap").as_bool() != Some(false)) != app.ui.chat.timeline.wrapped.contains(&key);
    let wrap_key = key.clone();
    let tools = row()
        .gap(2.0)
        .center_y()
        .role("toolbar")
        .label("Code block actions")
        .child(
            row()
                .size(24.0, 24.0)
                .center()
                .child(icon(&icons::TEXT_WRAP, 12.0).color(t.c.muted_foreground))
                .radius(6.0)
                .fill(if wrap {
                    t.c.secondary
                } else {
                    rune::Color::TRANSPARENT
                })
                .hover(t.c.muted)
                .color(t.c.muted_foreground)
                .focusable(true)
                .role("button")
                .label(if wrap { "Disable line wrap" } else { "Wrap lines" })
                .help(if wrap { "Disable line wrap" } else { "Wrap lines" })
                .on_press(move |app: &mut App| {
                    if !app.ui.chat.timeline.wrapped.remove(&wrap_key) {
                        app.ui.chat.timeline.wrapped.insert(wrap_key.clone());
                    }
                }),
        )
        .children((cx.run && runnable(language, code, closed)).then(|| {
            let command = code.trim().to_owned();
            row()
                .size(24.0, 24.0)
                .center()
                .child(icon(&icons::PLAY, 12.0).color(t.c.muted_foreground))
                .radius(6.0)
                .hover(t.c.muted)
                .focusable(true)
                .role("button")
                .label("Run in terminal")
                .help("Run in terminal")
                .on_press(move |app: &mut App| crate::chat::details::scripts::run_command(app, &command))
        }))
        .child(copy_button(app, code, t, "Copy code"));
    let head = row()
        .w_full()
        .gap(8.0)
        .pl(12.0)
        .pr(6.0)
        .pt(7.0)
        .child(row().grow(1.0).child(language_icons::language(language, t)))
        .child(tools);
    let fenced = if language == "mermaid" && !closed {
        format!("```{language}\n{code}")
    } else {
        format!("```{language}\n{code}\n```")
    };
    let theme = *t;
    let code_view = Markdown::new(fenced.as_str())
        .selectable(true)
        .highlights(marks_for(cx.marks, &fenced))
        .scroll_wide(!wrap)
        .streaming(language == "mermaid" && !closed)
        .diagram_frame(move |card, frame| {
            let source = frame.source.to_owned();
            col().w_full().child(card).child(row().w_full().justify_end().px(6.0).pb(6.0).child(
                crate::kit::Button::new("Expand diagram").variant("ghost").size("icon-sm").icon(&icons::MAXIMIZE_2)
                    .on_press(move |s: &mut App| s.ui.chat.timeline.expanded_diagram = Some(source.clone()))
                    .build(&theme),
            ))
        })
        .render(
            &style(t, cx.ink)
                .code_block(rune::Color::TRANSPARENT, rune::Color::TRANSPARENT)
                .code_block_padding(15.4, 11.8),
        );
    col()
        .key(key)
        .w_full()
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(code_background(t))
        .border(
            1.0,
            if t.dark {
                rune::Color::TRANSPARENT
            } else {
                t.c.border.alpha_mul(0.7)
            },
        )
        .child(head)
        .child(code_view)
}

/// A GitHub alert (`> [!NOTE]`), GITHUB_ALERT_PRESENTATIONS.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Alert {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

impl Alert {
    fn parse(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "note" => Self::Note,
            "tip" => Self::Tip,
            "important" => Self::Important,
            "warning" => Self::Warning,
            "caution" => Self::Caution,
            _ => return None,
        })
    }

    /// The label, its icon, the rule's color and the title's.
    fn look(self, dark: bool) -> (&'static str, &'static rune::Icon, rune::Color, rune::Color) {
        use crate::theme::tw;
        let pick = |light, dark_ink| if dark { dark_ink } else { light };
        match self {
            Self::Note => ("Note", &icons::INFO, tw::BLUE_500, pick(tw::BLUE_600, tw::BLUE_400)),
            Self::Tip => ("Tip", &icons::LIGHTBULB, tw::EMERALD_500, pick(tw::EMERALD_600, tw::EMERALD_400)),
            Self::Important => ("Important", &icons::MESSAGE_SQUARE_WARNING, tw::PURPLE_500, pick(tw::PURPLE_600, tw::PURPLE_400)),
            Self::Warning => ("Warning", &icons::TRIANGLE_ALERT, tw::AMBER_500, pick(tw::AMBER_600, tw::AMBER_500)),
            Self::Caution => ("Caution", &icons::OCTAGON_ALERT, tw::RED_500, pick(tw::RED_600, tw::RED_400)),
        }
    }
}

/// ChatMarkdown's `blockquote` for an alert: a colored rule, its icon and label over the body.
fn alert(kind: Alert, body: El, t: &T) -> El {
    let (label, glyph, rule, ink) = kind.look(t.dark);
    row()
        .w_full()
        .py(4.0)
        .role("note")
        .child(el().w(2.0).self_stretch().fill(rule.alpha_mul(0.7)))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .pl(12.0)
                .gap(4.0)
                .child(row().gap(6.0).center_y().child(icon(glyph, 14.0).color(ink)).child(text(label).font(theme::weight(theme::TEXT_SM.lh(22.75), theme::MEDIUM)).color(ink)))
                .child(body),
        )
}

/// MarkdownDetails: the summary between rules, pressed to show or hide what it holds.
fn disclosure(summary: &str, key: String, open: bool, body: Option<El>, t: &T) -> El {
    let rule = || el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6));
    let toggle = key.clone();
    let head = row()
        .w_full()
        .gap(8.0)
        .py(8.0)
        .center_y()
        .cursor(rune::Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label(summary.to_owned())
        .on_press(move |app: &mut App| super::toggle(app, &toggle))
        .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.0).color(t.c.muted_foreground))
        .child(text(summary.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).shrink().min_w(0.0));
    col()
        .key(key)
        .w_full()
        .child(rule())
        .child(head)
        .children(body.map(|b| col().w_full().pl(24.0).pb(12.0).child(b)))
        .child(rule())
}

/// CodexArtifactTemplateCard: the template's kind icon, name and kind, and "Use template".
fn template_card(template: &Template, t: &T) -> El {
    let badge = el()
        .pin(None, None, Some(-4.0), Some(-4.0))
        .size(16.0, 16.0)
        .center()
        .radius(theme::radius::FULL)
        .border(1.0, t.c.background)
        .fill(t.c.primary)
        .child(icon(&icons::SPARKLES, 10.0).color(t.c.primary_foreground));
    let tile = el()
        .size(36.0, 36.0)
        .center()
        .radius(8.0)
        .border(1.0, t.c.border.alpha_mul(0.7))
        .fill(t.c.background)
        .child(icon(template.icon(), 20.0).color(t.c.muted_foreground))
        .child(badge);
    let chosen = template.clone();
    row()
        .w_full()
        .gap(12.0)
        .center_y()
        .px(12.0)
        .py(10.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.7))
        .fill(t.c.card.alpha_mul(0.6))
        .role("group")
        .label(format!("{} template", template.display_name))
        .child(
            row()
                .grow(1.0)
                .min_w(0.0)
                .gap(12.0)
                .center_y()
                .child(tile)
                .child(
                    col()
                        .shrink()
                        .min_w(0.0)
                        .child(text(template.display_name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                        .child(text(template.kind_label()).font(theme::TEXT_XS).color(t.c.muted_foreground)),
                ),
        )
        .child(crate::kit::Button::new("Use template").variant("outline").size("sm").build(t).on_press(move |app: &mut App| use_template(app, &chosen)))
}

/// `insertArtifactTemplate`: the template's prompt at the end of the draft, unless it already
/// ends with it.
fn use_template(app: &mut App, template: &Template) {
    let prompt = template.use_prompt();
    let route = crate::chat::composer::route(app);
    let key = crate::chat::composer::draft_key(app, &route);
    let draft = app.ui.chat.composer.drafts.get(&key).map(|d| d.edit.text.clone()).unwrap_or_default();
    let trimmed = draft.trim_end();
    let ends_with_prompt = trimmed.strip_suffix(prompt.as_str()).is_some_and(|before| before.is_empty() || before.ends_with(char::is_whitespace));
    if !ends_with_prompt {
        let gap = if draft.is_empty() || draft.ends_with(char::is_whitespace) { "" } else { " " };
        crate::chat::composer::append_with_chips(app, &prompt, &[], gap);
    }
    crate::chat::composer::focus(app, &route);
}

/// A run of a message's Markdown that draws on its own.
#[derive(Debug, PartialEq)]
enum Part {
    /// Plain Markdown from line `start`, its Codex inline directives already rendered.
    Prose { start: usize, text: String },
    Fence { language: String, code: String, closed: bool },
    Alert { kind: Alert, body: String },
    Details { summary: String, open: bool, body: String },
    Template(Template),
}

fn fence_marker(line: &str) -> Option<&'static str> {
    let line = line.trim_start();
    ["```", "~~~"].into_iter().find(|m| line.starts_with(m))
}

/// A blockquote line's content past its `>` and one space.
fn quoted(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let rest = rest.strip_prefix('>')?;
    Some(rest.strip_prefix([' ', '\t']).unwrap_or(rest))
}

/// remarkGithubAlerts: a quote that opens with a line of just `[!NOTE]` (any case).
fn alert_marker(line: &str) -> Option<Alert> {
    let name = quoted(line)?.trim_end().strip_prefix("[!")?.strip_suffix(']')?;
    Alert::parse(name)
}

fn opens_details(line: &str) -> bool {
    let line = line.trim_start_matches(' ');
    line.get(..8).is_some_and(|tag| tag.eq_ignore_ascii_case("<details")) && line.as_bytes().get(8).is_some_and(|b| *b == b'>' || b.is_ascii_whitespace())
}

/// How many `<details>` a line opens less those it closes.
fn details_depth(line: &str) -> isize {
    let lower = line.to_ascii_lowercase();
    let opens = lower.match_indices("<details").filter(|(i, _)| lower.as_bytes().get(i + 8).is_some_and(|b| *b == b'>' || b.is_ascii_whitespace())).count();
    opens as isize - lower.matches("</details>").count() as isize
}

/// A `<details>` block's summary ("Details" without one), whether it is written open, and the
/// Markdown inside it.
fn details_block(block: &str) -> (String, bool, String) {
    let block = block.trim_start_matches(' ');
    let tag_end = block.find('>').unwrap_or(block.len());
    let open = block[8.min(tag_end)..tag_end].split_whitespace().any(|a| a.get(..4).is_some_and(|n| n.eq_ignore_ascii_case("open")) && matches!(a.as_bytes().get(4), None | Some(b'=')));
    let mut inner = block.get(tag_end + 1..).unwrap_or("");
    if let Some(close) = inner.to_ascii_lowercase().rfind("</details>") {
        inner = &inner[..close];
    }
    let lower = inner.to_ascii_lowercase();
    let (summary, body) = match (lower.find("<summary"), lower.find("</summary>")) {
        (Some(from), Some(to)) if from < to => {
            let text_from = inner[from..to].find('>').map_or(to, |k| from + k + 1);
            (plain_html(&inner[text_from..to]), format!("{}{}", &inner[..from], &inner[to + "</summary>".len()..]))
        }
        _ => (String::new(), inner.to_owned()),
    };
    let summary = if summary.trim().is_empty() { "Details".to_owned() } else { summary.trim().to_owned() };
    (summary, open, body.trim_start_matches(['\r', '\n']).to_owned())
}

/// HTML's text: tags dropped, the common entities read.
fn plain_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&nbsp;", " ").replace("&amp;", "&")
}

/// A message's blocks: fences, GitHub alerts, `<details>` and artifact templates split out, the
/// rest prose with its inline directives rendered. Runs inside the memoized build, so once per
/// message version.
fn parts(source: &str) -> Vec<Part> {
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut out = Vec::new();
    let flush = |out: &mut Vec<Part>, from: usize, to: usize| {
        let text = lines[from..to].concat();
        if !text.trim().is_empty() {
            out.push(Part::Prose { start: from, text: directives(&text).into_owned() });
        }
    };
    let (mut start, mut i) = (0, 0);
    while i < lines.len() {
        let line = lines[i];
        if let Some(marker) = fence_marker(line) {
            flush(&mut out, start, i);
            let language = line.trim_start().trim_start_matches(marker).split_whitespace().next().unwrap_or("").to_owned();
            let code_start = i + 1;
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(marker) {
                i += 1;
            }
            let closed = i < lines.len();
            let code = lines[code_start..i].concat().trim_end_matches('\n').to_owned();
            out.push(Part::Fence { language, code, closed });
            i = (i + 1).min(lines.len());
        } else if let Some(kind) = alert_marker(line) {
            flush(&mut out, start, i);
            let mut body = String::new();
            i += 1;
            while let Some(rest) = lines.get(i).and_then(|l| quoted(l)) {
                body.push_str(rest);
                i += 1;
            }
            out.push(Part::Alert { kind, body });
        } else if opens_details(line) {
            flush(&mut out, start, i);
            let from = i;
            let mut depth = 0;
            while i < lines.len() {
                depth += details_depth(lines[i]);
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
            let (summary, open, body) = details_block(&lines[from..i].concat());
            out.push(Part::Details { summary, open, body });
        } else if let Some(template) = template_line(line) {
            flush(&mut out, start, i);
            out.push(Part::Template(template));
            i += 1;
        } else {
            i += 1;
            continue;
        }
        start = i;
    }
    flush(&mut out, start, lines.len());
    out
}

const CITATION: &str = "codex-file-citation";
const FOLLOW_UP: &str = "codex-followup";
const TEMPLATE: &str = "artifact-template";

/// An artifact template a Codex reply offers (CodexArtifactTemplate).
#[derive(Clone, Debug, PartialEq)]
struct Template {
    kind: &'static str,
    display_name: String,
    skill_name: String,
}

/// Each artifact kind, its presentation label, its icon, and its use prompt around the skill.
static TEMPLATE_KINDS: [(&str, &str, &rune::Icon, &str, &str); 10] = [
    ("document", "Document template", &icons::FILE_TEXT, "Create a document using this ", " about…"),
    ("presentation", "Presentation template", &icons::PRESENTATION, "Create a presentation using the ", " template about…"),
    ("spreadsheet", "Spreadsheet template", &icons::FILE_SPREADSHEET, "Create a spreadsheet using this ", " about…"),
    ("site", "Site template", &icons::GLOBE, "Create a Site using this ", " about…"),
    ("google-docs", "Google Doc template", &icons::FILE_TEXT, "Create a Google Doc using this ", " about…"),
    ("google-slides", "Google Slides template", &icons::PRESENTATION, "Create a Google Slides presentation using this ", " about…"),
    ("google-sheets", "Google Sheet template", &icons::FILE_SPREADSHEET, "Create a Google Sheet using this ", " about…"),
    ("image", "Image template", &icons::IMAGE, "Create an image using this ", " of…"),
    ("email", "Email template", &icons::MAIL, "Draft an email using this ", " about…"),
    ("slack", "Slack template", &icons::MESSAGE_SQUARE, "Draft a Slack message using this ", " about…"),
];

impl Template {
    fn entry(&self) -> &'static (&'static str, &'static str, &'static rune::Icon, &'static str, &'static str) {
        TEMPLATE_KINDS.iter().find(|k| k.0 == self.kind).unwrap_or(&TEMPLATE_KINDS[0])
    }

    /// codexArtifactTemplatePresentationLabel.
    fn kind_label(&self) -> &'static str {
        self.entry().1
    }

    fn icon(&self) -> &'static rune::Icon {
        self.entry().2
    }

    /// codexArtifactTemplateUsePrompt.
    fn use_prompt(&self) -> String {
        let (_, _, _, before, after) = self.entry();
        format!("{before}${}{after}", self.skill_name)
    }

    /// resolveCodexArtifactTemplate.
    fn resolve(attrs: &[(String, String)]) -> Option<Self> {
        let kind = TEMPLATE_KINDS.iter().find(|k| Some(k.0) == attr(attrs, "artifact_kind"))?.0;
        let display_name = attr(attrs, "display_name")?.trim();
        let directory = attr(attrs, "skill_directory")?;
        let skill_name = attr(attrs, "skill_name")?;
        let gallery_ok = attr(attrs, "gallery_kind").is_none_or(|g| matches!(g, "imagegen" | "product-design"));
        (!display_name.is_empty() && absolute_directory(directory) && skill_name.starts_with("artifact-template-") && gallery_ok)
            .then(|| Template { kind, display_name: display_name.to_owned(), skill_name: skill_name.to_owned() })
    }
}

fn absolute_directory(path: &str) -> bool {
    let b = path.as_bytes();
    let drive = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/');
    let unc = |sep: char| {
        let prefix = if sep == '\\' { "\\\\" } else { "//" };
        path.strip_prefix(prefix).is_some_and(|rest| {
            let mut parts = rest.splitn(3, sep);
            parts.next().is_some_and(|p| !p.is_empty()) && parts.next().is_some_and(|p| !p.is_empty())
        })
    };
    (path.starts_with('/') && !path.starts_with("//")) || drive || unc('\\') || unc('/')
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().rev().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

/// A line holding only an `::artifact-template{…}` that resolves.
fn template_line(line: &str) -> Option<Template> {
    let line = line.trim_end_matches(['\n', '\r']);
    let rest = line.trim_start_matches(' ');
    if line.len() - rest.len() > 3 {
        return None;
    }
    let tail = rest.strip_prefix("::")?.strip_prefix(TEMPLATE)?;
    let (_, attrs, used) = directive_tail(tail)?;
    if !tail[used..].trim().is_empty() {
        return None;
    }
    Template::resolve(&attrs?)
}

/// The byte past the `]` closing the `[` at `open`, escapes skipped and nesting counted; none
/// past a blank line.
fn closing_bracket(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0;
    let mut i = open;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 1,
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'\n' if b.get(i + 1) == Some(&b'\n') => return None,
            _ => {}
        }
        i += 1;
    }
    None
}

/// A directive's tail after its name: the `[label]` and `{attributes}` it has, and how many
/// bytes they take. `None` when the name runs on (`:codex-followup-extra`).
fn directive_tail(s: &str) -> Option<(Option<&str>, Option<Vec<(String, String)>>, usize)> {
    if s.chars().next().is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let mut used = 0;
    let mut label = None;
    if s.starts_with('[') {
        match closing_bracket(s.as_bytes(), 0) {
            Some(close) => {
                label = Some(&s[1..close]);
                used = close + 1;
            }
            None => return Some((None, None, 0)),
        }
    }
    let attrs = match s[used..].starts_with('{').then(|| attributes(&s[used..])).flatten() {
        Some((attrs, n)) => {
            used += n;
            Some(attrs)
        }
        None => None,
    };
    Some((label, attrs, used))
}

/// A directive's `{key="value" key=value key #id .class}` and the bytes it takes, values raw.
fn attributes(s: &str) -> Option<(Vec<(String, String)>, usize)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    let key_byte = |c: u8| c.is_ascii_alphanumeric() || matches!(c, b'_' | b':' | b'.' | b'-');
    let bare_byte = |c: u8| !c.is_ascii_whitespace() && !matches!(c, b'"' | b'\'' | b'=' | b'<' | b'>' | b'`' | b'}');
    loop {
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        match *b.get(i)? {
            b'}' => return Some((out, i + 1)),
            b'#' | b'.' => {
                let name = if b[i] == b'#' { "id" } else { "class" };
                let from = i + 1;
                i = from;
                while i < b.len() && bare_byte(b[i]) {
                    i += 1;
                }
                if i == from {
                    return None;
                }
                out.push((name.to_owned(), s[from..i].to_owned()));
            }
            _ => {
                let from = i;
                while i < b.len() && key_byte(b[i]) {
                    i += 1;
                }
                if i == from {
                    return None;
                }
                let key = s[from..i].to_owned();
                let mut j = i;
                while j < b.len() && matches!(b[j], b' ' | b'\t') {
                    j += 1;
                }
                if b.get(j) != Some(&b'=') {
                    out.push((key, String::new()));
                    continue;
                }
                i = j + 1;
                while i < b.len() && matches!(b[i], b' ' | b'\t') {
                    i += 1;
                }
                let value = match *b.get(i)? {
                    q @ (b'"' | b'\'') => {
                        let close = i + 1 + s[i + 1..].find(q as char)?;
                        let value = &s[i + 1..close];
                        i = close + 1;
                        value
                    }
                    _ => {
                        let from = i;
                        while i < b.len() && bare_byte(b[i]) {
                            i += 1;
                        }
                        if i == from {
                            return None;
                        }
                        &s[from..i]
                    }
                };
                out.push((key, value.to_owned()));
            }
        }
    }
}

/// `positiveInteger`: a whole number above zero.
fn positive(value: &str) -> Option<u64> {
    let n: f64 = value.trim().parse().ok()?;
    (n > 0.0 && n.fract() == 0.0 && n <= 9_007_199_254_740_991.0).then_some(n as u64)
}

/// `codexFileCitationMarkdown`: `[basename](<path#Lline>)`, the label escaped and the path
/// encoded so Markdown reads it back unchanged.
fn citation_markdown(path: &str, line: Option<&str>) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = normalized.trim_end_matches('/');
    let base = normalized.rsplit('/').next().unwrap_or("");
    let label = [base, normalized, "File"].into_iter().find(|v| !v.is_empty()).unwrap_or("File");
    let mut href = path.replace('%', "%25").replace('#', "%23").replace('?', "%3F");
    if let Some(n) = line.and_then(positive) {
        href.push_str(&format!("#L{n}"));
    }
    let mut escaped = String::with_capacity(label.len());
    for c in label.chars() {
        if matches!(c, '\\' | '[' | ']' | '*' | '_' | '`' | '<' | '&') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    let dest = href.replace('\\', "%5C").replace('<', "%3C").replace('>', "%3E").replace('\r', "%0D").replace('\n', "%0A");
    format!("[{escaped}](<{dest}>)")
}

/// The Markdown a text directive at `at` (its `:`) shows as, and the byte past it: a file
/// citation's file link (not inside a link's text) or a follow-up's label. Malformed ones stay.
fn directive_at(text: &str, at: usize, in_link: bool) -> Option<(usize, String)> {
    let rest = &text[at + 1..];
    let name = [CITATION, FOLLOW_UP].into_iter().find(|n| rest.starts_with(n))?;
    let tail = at + 1 + name.len();
    let (label, attrs, used) = directive_tail(&text[tail..])?;
    let attrs = attrs?;
    let end = tail + used;
    if name == CITATION {
        let path = attr(&attrs, "path")?.trim();
        return (!in_link && !path.is_empty()).then(|| (end, citation_markdown(path, attr(&attrs, "line_range_start"))));
    }
    let label = label?;
    let has_text = label.chars().any(|c| !c.is_whitespace() && !matches!(c, '*' | '_' | '`' | '~'));
    (has_text && attr(&attrs, "prompt").is_some_and(|p| !p.trim().is_empty())).then(|| (end, label.to_owned()))
}

/// `renderCodexInlineDirectivesAsMarkdown` on fence-free Markdown: file citations as file links,
/// follow-ups as their labels. Those in code spans, escaped, or (citations) in a link's text
/// stay as written.
fn directives(text: &str) -> Cow<'_, str> {
    if !text.contains(":codex-") {
        return Cow::Borrowed(text);
    }
    let b = text.as_bytes();
    let mut out = String::new();
    let (mut i, mut copied, mut link_end) = (0, 0, 0);
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'`' => {
                let run = b[i..].iter().take_while(|c| **c == b'`').count();
                let fence = &text[i..i + run];
                let mut close = None;
                let mut j = i + run;
                while let Some(k) = text[j..].find(fence).map(|k| j + k) {
                    let after = k + run;
                    if b.get(after) != Some(&b'`') {
                        close = Some(after);
                        break;
                    }
                    j = after + b[after..].iter().take_while(|c| **c == b'`').count();
                }
                i = close.unwrap_or(i + run);
            }
            b'[' if i >= link_end => {
                if let Some(close) = closing_bracket(b, i)
                    && matches!(b.get(close + 1), Some(b'(' | b'['))
                {
                    link_end = close;
                }
                i += 1;
            }
            b':' if i == 0 || b[i - 1] != b':' => match directive_at(text, i, i < link_end) {
                Some((end, shown)) => {
                    out.push_str(&text[copied..i]);
                    out.push_str(&shown);
                    copied = end;
                    i = end;
                }
                None => i += 1,
            },
            _ => i += 1,
        }
    }
    if copied == 0 {
        return Cow::Borrowed(text);
    }
    out.push_str(&text[copied..]);
    Cow::Owned(out)
}

/// `renderCodexDirectivesForCopy`: a message as Copy writes it, its Codex directives as they
/// show (file links, follow-up labels, "Name (Kind template)"); fences stay as written.
pub fn for_copy(text: &str) -> Cow<'_, str> {
    if !text.contains(":codex-") && !text.contains("::artifact-template") {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut run = String::new();
    let mut fence: Option<&str> = None;
    for line in text.split_inclusive('\n') {
        if let Some(open) = fence {
            out.push_str(line);
            if line.trim_start().starts_with(open) {
                fence = None;
            }
            continue;
        }
        let template = template_line(line);
        let marker = fence_marker(line);
        if template.is_none() && marker.is_none() {
            run.push_str(line);
            continue;
        }
        out.push_str(&directives(&run));
        run.clear();
        match template {
            Some(template) => {
                out.push_str(&format!("{} ({})", template.display_name, template.kind_label()));
                if line.ends_with('\n') {
                    out.push('\n');
                }
            }
            None => {
                fence = marker;
                out.push_str(line);
            }
        }
    }
    out.push_str(&directives(&run));
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::{Alert, Part, Template, citation_markdown, directives, for_copy, parts, runnable, user_source};
    use crate::chat::timeline::md_links::{Target, classify};

    #[test]
    fn only_a_closed_one_line_shell_fence_runs() {
        assert!(runnable("bash", "npm test", true));
        assert!(runnable("pwsh", "  Get-ChildItem  ", true));
        assert!(!runnable("bash", "npm test", false), "FAILED: a fence still streaming");
        assert!(!runnable("python", "print(1)", true));
        assert!(!runnable("sh", "   ", true));
        assert!(!runnable("sh", "make \\", true), "FAILED: a continuation waits for more input");
        assert!(!runnable("sh", "cd a\nmake", true), "FAILED: a newline is a control character");
        assert!(!runnable("sh", "rm -rf \u{202e}fdsa", true), "FAILED: a bidi override hides the real command");
        assert!(!runnable("sh", "ls\u{200b}", true));
    }

    const FILE_CITATION: &str = ":codex-file-citation{path=\"outputs/report.xlsx\" purpose=\"output\"}";
    const FOLLOW_UP: &str = ":codex-followup[Prepare print version]{prompt=\"Prepare the document for printing.\"}";
    const ARTIFACT: &str = "::artifact-template{skill_name=\"artifact-template-hello-world\" skill_directory=\"/Users/test/.codex/skills/artifact-template-hello-world\" display_name=\"Hello World\" artifact_kind=\"document\"}";

    #[test]
    fn user_text_keeps_its_line_breaks_outside_code() {
        assert_eq!(user_source("a\nb"), "a  \nb");
        assert_eq!(user_source("a\n\nb\n"), "a  \n\nb  \n");
        assert_eq!(user_source("x\n```\nfn a()\nfn b()\n```\ny"), "x  \n```\nfn a()\nfn b()\n```\ny");
        assert_eq!(user_source("~~~\n```\n~~~\nz"), "~~~\n```\n~~~\nz");
    }

    #[test]
    fn alerts_and_details_split_out_of_prose() {
        let source = "Intro\n> [!warning]\n> Back up first.\n>\n> Then run it.\n\n<details open>\n<summary>Logs <b>(3)</b></summary>\n\n```sh\nls\n```\n</details>\nAfter\n";
        assert_eq!(parts(source), vec![
            Part::Prose { start: 0, text: "Intro\n".into() },
            Part::Alert { kind: Alert::Warning, body: "Back up first.\n\nThen run it.\n".into() },
            Part::Details { summary: "Logs (3)".into(), open: true, body: "```sh\nls\n```\n".into() },
            Part::Prose { start: 13, text: "After\n".into() },
        ]);
        // Only a marker alone on its line makes an alert; other quotes stay Markdown.
        for quote in ["> [!NOTE] inline", "> plain quote", "> [!TODO]"] {
            assert_eq!(parts(quote), vec![Part::Prose { start: 0, text: quote.into() }]);
        }
        assert_eq!(parts(">[!Tip]"), vec![Part::Alert { kind: Alert::Tip, body: String::new() }]);
        // A summary defaults to "Details"; an unclosed block runs to the end, as it streams.
        assert_eq!(parts("<details>\nhidden\n</details>"), vec![Part::Details { summary: "Details".into(), open: false, body: "hidden\n".into() }]);
        assert_eq!(parts("<details>\n<summary>S</summary>\nbody"), vec![Part::Details { summary: "S".into(), open: false, body: "body".into() }]);
        let nested = "<details><summary>Outer</summary>\n\n<details><summary>Inner</summary>\nx\n</details>\n</details>\ntail";
        let Part::Details { summary, body, .. } = &parts(nested)[0] else { panic!("not details") };
        assert_eq!((summary.as_str(), body.as_str()), ("Outer", "<details><summary>Inner</summary>\nx\n</details>\n"));
        assert_eq!(parts(nested)[1], Part::Prose { start: 6, text: "tail".into() });
    }

    #[test]
    fn fences_split_out_closed_or_still_streaming() {
        assert_eq!(parts("```bash\nnpm test\n```")[0], Part::Fence { language: "bash".into(), code: "npm test".into(), closed: true });
        assert_eq!(parts("```sh\nls")[0], Part::Fence { language: "sh".into(), code: "ls".into(), closed: false });
    }

    #[test]
    fn codex_directives_render_as_their_markdown() {
        assert_eq!(directives(&format!("Created {FILE_CITATION}.")), "Created [report.xlsx](<outputs/report.xlsx>).");
        assert_eq!(
            directives(&format!("- {FOLLOW_UP}\n- :codex-followup[Review **document**]{{prompt=\"Review it.\"}}")),
            "- Prepare print version\n- Review **document**"
        );
        for kept in [
            "Meeting at 10:30".to_owned(),
            ":::note\ncontent\n:::".to_owned(),
            ":codex-file-citation-extra{path=\"outputs/report.xlsx\"}".to_owned(),
            ":codex-followup-extra[Label]{prompt=\"Prompt\"}".to_owned(),
            ":codex-file-citation{purpose=\"output\"}".to_owned(),
            ":codex-followup[Prepare print version]".to_owned(),
            ":codex-followup[ ]{prompt=\"Prepare the document for printing.\"}".to_owned(),
            format!("\\{FILE_CITATION}"),
            format!("`{FILE_CITATION}`"),
            format!("[See {FILE_CITATION}](https://example.com)"),
        ] {
            assert_eq!(directives(&kept), kept);
        }
        // regression: #12243 copied Codex citations keep their backslashes
        for path in ["C:\\Users\\test\\[draft]\\report.md", "/tmp/report%5C.md"] {
            let markdown = citation_markdown(path, Some("7"));
            let dest = markdown.split_once("](<").and_then(|(_, d)| d.strip_suffix(">)")).unwrap();
            assert_eq!(classify(dest), Target::File { path: path.to_owned(), line: Some(7) });
        }
        assert_eq!(citation_markdown("a/b_c*.md", Some("0")), "[b\\_c\\*.md](<a/b_c*.md>)");
    }

    #[test]
    fn artifact_templates_become_cards_and_copy_as_their_names() {
        let template = Template { kind: "document", display_name: "Hello World".into(), skill_name: "artifact-template-hello-world".into() };
        assert_eq!(parts(&format!("Before\n\n{ARTIFACT}\n\nAfter")), vec![
            Part::Prose { start: 0, text: "Before\n\n".into() },
            Part::Template(template.clone()),
            Part::Prose { start: 3, text: "\nAfter".into() },
        ]);
        assert_eq!(template.use_prompt(), "Create a document using this $artifact-template-hello-world about…");
        let malformed = "::artifact-template{display_name=\"Hello World\"}";
        assert_eq!(parts(malformed), vec![Part::Prose { start: 0, text: malformed.into() }]);
        assert_eq!(
            for_copy(&format!("Created {FILE_CITATION}.\n\n{ARTIFACT}\n\n- {FOLLOW_UP}")),
            "Created [report.xlsx](<outputs/report.xlsx>).\n\nHello World (Document template)\n\n- Prepare print version"
        );
        let fenced = format!("```\n{FILE_CITATION}\n```\n");
        assert_eq!(for_copy(&fenced), fenced);
        assert_eq!(for_copy("plain"), "plain");
    }
}
