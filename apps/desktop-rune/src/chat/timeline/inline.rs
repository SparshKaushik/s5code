//! A Markdown paragraph drawn as one text with pictures in its lines (Rune's `InlinePicture`):
//! KaTeX formulas, and images, which web draws inline-block in prose. The text wraps, selects
//! and copies as one, each picture copying as its source. Bold, italic, code and links keep
//! their look; anything richer stays with rune_kit's Markdown (`None`).

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use rune::tok::TextStyle;
use rune::{Color, El, InlinePicture, Link, PICTURE_MARK, Span, Tx, text};

use crate::App;
use crate::theme::{self, AlphaMul, T};

pub(super) struct Picture {
    pub pixels: rune::Pixels,
    /// In points.
    pub size: [f32; 2],
    pub copy: String,
    /// The media key pressing it opens in the lightbox.
    pub open: Option<String>,
    /// Points from its top to the text baseline; `None` centres it on the line.
    pub baseline: Option<f32>,
    /// Web's media frame: rounded, with a faint border.
    pub framed: bool,
}

/// Link ids that open a picture in the lightbox rather than go anywhere.
const MEDIA: &str = "rune-media:";

/// Where formula `index` goes in the source handed to [`paragraph`].
pub(super) fn placeholder(index: usize) -> String {
    format!("\u{e000}{index}\u{e001}")
}

/// Lines a paragraph layout would draw wrong: headings, quotes, lists, tables and fences.
pub(super) fn block_syntax(line: &str) -> bool {
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    line.starts_with(['#', '>', '|', '<', '-', '*', '+', '`', '~', '='])
        || (digits > 0 && (line[digits..].starts_with(". ") || line[digits..].starts_with(") ")))
}

enum Open {
    Strong,
    Emphasis,
    Link(String),
}

/// `source` with `formulas` at their placeholders and each image `image` resolves; `None`
/// when a formula or image isn't ready, or for Markdown this text can't draw.
pub(super) fn paragraph(source: &str, mut formulas: Vec<Option<Picture>>, image: &dyn Fn(&str, &str) -> Option<Picture>, font: TextStyle, ink: Color, t: &T) -> Option<El> {
    if source.lines().any(|l| block_syntax(l.trim_start())) {
        return None;
    }
    let link_ink = t.c.info_foreground;
    let frame = t.c.border.alpha_mul(0.4);
    let mut content = String::new();
    let mut spans: Vec<Span> = Vec::new();
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut pictures: Vec<InlinePicture> = Vec::new();
    let mut said = String::new();
    let mut open: Vec<(usize, Open)> = Vec::new();
    let mut alt: Option<(String, String)> = None;
    let mut place = |content: &mut String, said: &mut String, links: &mut Vec<_>, picture: Picture| {
        let at = content.len();
        content.push(PICTURE_MARK);
        said.push_str(&picture.copy);
        if let Some(key) = picture.open {
            links.push((at..content.len(), format!("{MEDIA}{key}")));
        }
        let mut shown = InlinePicture::new(at, picture.pixels, picture.size[0], picture.size[1]).copy_as(picture.copy);
        if let Some(baseline) = picture.baseline {
            shown = shown.baseline(baseline);
        }
        if picture.framed {
            shown = shown.radius(theme::radius::LG).border(1.0, frame);
        }
        pictures.push(shown);
    };
    for event in Parser::new(source) {
        match event {
            Event::Start(Tag::Paragraph) | Event::End(TagEnd::Paragraph) => {}
            Event::Start(Tag::Image { dest_url, .. }) if alt.is_none() => alt = Some((String::new(), dest_url.into_string())),
            Event::End(TagEnd::Image) => {
                let (name, src) = alt.take()?;
                let picture = image(&name, &src)?;
                place(&mut content, &mut said, &mut links, picture);
            }
            Event::Text(words) | Event::Code(words) if alt.is_some() => alt.as_mut()?.0.push_str(&words),
            Event::SoftBreak if alt.is_some() => alt.as_mut()?.0.push(' '),
            Event::Start(Tag::Strong) => open.push((content.len(), Open::Strong)),
            Event::Start(Tag::Emphasis) => open.push((content.len(), Open::Emphasis)),
            Event::Start(Tag::Link { dest_url, .. }) => open.push((content.len(), Open::Link(dest_url.into_string()))),
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Link) => {
                let (start, what) = open.pop()?;
                let span = Span::new(start, content.len());
                match what {
                    Open::Strong => spans.push(span.weight(600.0)),
                    Open::Emphasis => spans.push(span.italic()),
                    Open::Link(url) => {
                        spans.push(span.color(link_ink));
                        links.push((start..content.len(), url));
                    }
                }
            }
            Event::Text(words) => {
                let mut rest: &str = &words;
                while let Some(at) = rest.find('\u{e000}') {
                    content.push_str(&rest[..at]);
                    said.push_str(&rest[..at]);
                    let close = rest[at..].find('\u{e001}').map(|c| at + c)?;
                    let index: usize = rest[at + '\u{e000}'.len_utf8()..close].parse().ok()?;
                    let picture = formulas.get_mut(index)?.take()?;
                    place(&mut content, &mut said, &mut links, picture);
                    rest = &rest[close + '\u{e001}'.len_utf8()..];
                }
                content.push_str(rest);
                said.push_str(rest);
            }
            Event::Code(code) => {
                let start = content.len();
                content.push_str(&code);
                said.push_str(&code);
                spans.push(Span::new(start, content.len()).mono().size(font.size * 0.85).bg(t.c.foreground.alpha_mul(0.1), 4.0));
            }
            Event::SoftBreak => {
                content.push(' ');
                said.push(' ');
            }
            Event::HardBreak => {
                content.push('\n');
                said.push('\n');
            }
            _ => return None,
        }
    }
    if pictures.is_empty() {
        return None;
    }
    let gallery: Vec<String> = links.iter().filter_map(|(_, url)| url.strip_prefix(MEDIA).map(str::to_owned)).collect();
    let mut shown = text(content).font(font).color(ink).wrap().selectable(true).spans(spans).label(said);
    for (range, url) in links {
        let help = match url.strip_prefix(MEDIA) {
            Some(_) => Some("Preview image".to_owned()),
            None => super::md_links::help(&url),
        };
        let link = Link::new(range, url);
        shown = shown.link(match help {
            Some(help) => link.help(help),
            None => link,
        });
    }
    shown = shown.on_link(move |app: &mut App, url: &str, tx: &mut Tx| match url.strip_prefix(MEDIA) {
        Some(key) => {
            let assets = &mut app.ui.chat.timeline.assets;
            assets.open = Some(key.to_owned());
            assets.gallery = gallery.clone();
        }
        None => super::md_links::open(app, url, tx),
    });
    Some(pictures.into_iter().fold(shown, El::picture))
}
