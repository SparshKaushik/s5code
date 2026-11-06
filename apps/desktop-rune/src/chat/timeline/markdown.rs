use super::copy_button;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, icon, row, text};
use rune_kit::markdown::{Markdown, MarkdownStyle};

fn style(t: &T) -> MarkdownStyle {
    MarkdownStyle::neutral(t.dark)
        .text(t.c.foreground.alpha_mul(0.9))
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
        .code_block(t.c.code_background, t.c.border)
        .code_block_font(theme::MONO_XS.lh(18.0))
        .code_block_padding(14.4, 12.8)
        .table_outline(None)
        .table_rules(t.c.border)
        .table_header_fill(t.c.background)
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

fn prose(source: &str, t: &T) -> El {
    let mut out = col().w_full().gap(10.4);
    for (index, block) in source
        .trim()
        .split("\n\n")
        .filter(|s| !s.trim().is_empty())
        .enumerate()
    {
        let mut content = Markdown::new(block)
            .selectable(true)
            .on_link_tx(|_: &mut App, url, tx| {
                let _ = tx.open_url(url);
            })
            .render(&style(t));
        if index > 0 && block.starts_with('#') {
            content = col().pt(9.6).child(content);
        }
        if block
            .lines()
            .nth(1)
            .is_some_and(|line| line.contains("---") && line.contains('|'))
        {
            content = col().w_full().child(content).child(
                row()
                    .w_full()
                    .justify_end()
                    .pb(16.0)
                    .child(copy_button(block, t)),
            );
        }
        out = out.child(content);
    }
    out
}

pub fn view(app: &App, source: &str, id: &str, t: &T) -> El {
    let mut out = col().w_full().gap(10.4);
    let lines: Vec<_> = source.split_inclusive('\n').collect();
    let mut start = 0;
    let mut i = 0;
    let mut fence_index = 0;
    while i < lines.len() {
        let line = lines[i].trim_start();
        let marker = if line.starts_with("```") {
            "```"
        } else if line.starts_with("~~~") {
            "~~~"
        } else {
            i += 1;
            continue;
        };
        if i > start {
            out = out.child(prose(&lines[start..i].concat(), t));
        }
        let language = line
            .trim_start_matches(marker)
            .trim()
            .split_whitespace()
            .next()
            .unwrap_or("");
        let code_start = i + 1;
        i += 1;
        while i < lines.len() && !lines[i].trim_start().starts_with(marker) {
            i += 1;
        }
        let code = lines[code_start..i]
            .concat()
            .trim_end_matches('\n')
            .to_owned();
        let key = format!("{id}-fence-{fence_index}");
        let wrap = app.ui.chat.timeline.wrapped.contains(&key);
        let wrap_key = key.clone();
        let head = row()
            .w_full()
            .gap(8.0)
            .px(12.0)
            .py(6.0)
            .fill(t.c.code_background)
            .child(
                text(language)
                    .font(theme::MONO_XS)
                    .color(t.c.muted_foreground)
                    .grow(1.0),
            )
            .child(
                icon(&icons::TEXT_WRAP, 12.0)
                    .padding(6.0)
                    .color(t.c.muted_foreground)
                    .focusable(true)
                    .role("button")
                    .label(if wrap { "Unwrap code" } else { "Wrap code" })
                    .on_press(move |app: &mut App| {
                        if !app.ui.chat.timeline.wrapped.remove(&wrap_key) {
                            app.ui.chat.timeline.wrapped.insert(wrap_key.clone());
                        }
                    }),
            )
            .child(copy_button(&code, t));
        let fenced = format!("```{language}\n{code}\n```");
        let code_view = Markdown::new(fenced)
            .selectable(true)
            .scroll_wide(!wrap)
            .render(&style(t).code_block(t.c.code_background, rune::Color::TRANSPARENT));
        out = out.child(
            col()
                .key(key)
                .w_full()
                .radius(8.0)
                .clip_rounded(8.0)
                .fill(t.c.code_background)
                .child(head)
                .child(code_view),
        );
        fence_index += 1;
        i = (i + 1).min(lines.len());
        start = i;
    }
    if start < lines.len() {
        out = out.child(prose(&lines[start..].concat(), t));
    }
    if source.is_empty() {
        out = out.child(icon(&icons::ELLIPSIS, 16.0).color(t.c.muted_foreground));
    }
    out
}
