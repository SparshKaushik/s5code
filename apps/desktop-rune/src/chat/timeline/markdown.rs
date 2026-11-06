use super::{copy_button, language_icons};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, icon, row};
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
        .code_block(code_background(t), rune::Color::TRANSPARENT)
        .code_block_font(theme::MONO_XS.lh(19.25))
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
            content = table(block, t);
        }
        out = out.child(content);
    }
    out
}

fn table(source: &str, t: &T) -> El {
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
                            .selectable(true)
                            .render(&style(t).body_font(font)),
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
            .pb(16.0)
            .child(copy_button(source, t)),
    )
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
        let wrap = !app.ui.chat.timeline.wrapped.contains(&key);
        let wrap_key = key.clone();
        let head = row()
            .w_full()
            .gap(8.0)
            .pl(12.0)
            .pr(6.0)
            .pt(6.0)
            .child(row().grow(1.0).child(language_icons::language(language, t)))
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
            .render(&style(t).code_block(rune::Color::TRANSPARENT, rune::Color::TRANSPARENT));
        out = out.child(
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
