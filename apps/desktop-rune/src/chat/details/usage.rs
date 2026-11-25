//! ThreadDetailsUsage and ThreadDetailsContext: the selected provider's limit windows as room
//! left, and the thread's context window, each a label, hairline and value row whose tooltip
//! holds the detail. Both are off until "Usage limits" or "Context window" is shown.

use rune::{El, col, el, row, text};

use crate::App;
use crate::theme::{self, AlphaMul, T};

#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Tone {
    Plain,
    Warning,
    Error,
}

/// One UsageRow: `fill` from 0 to 100 (`None` hides the bar), `tick` a marker on it.
#[derive(Debug, PartialEq)]
pub(crate) struct Row {
    pub label: String,
    pub fill: Option<f64>,
    pub tone: Tone,
    pub tick: Option<f64>,
    pub value: String,
    pub help: Vec<String>,
    pub muted: bool,
}

/// web's subgrid columns (label, bar, value), fixed so every row lines up in the 280px card.
const LABEL_W: f32 = 72.;
const BAR_W: f32 = 84.;
const VALUE_W: f32 = 64.;

fn row_view(t: &T, key: String, r: Row) -> El {
    let (bar_tone, value_tone) = match r.tone {
        Tone::Error => (t.c.error, t.c.error_foreground),
        Tone::Warning => (t.c.warning, t.c.warning_foreground),
        Tone::Plain if r.fill.is_none() => (t.c.foreground, t.c.muted_foreground),
        Tone::Plain => (if r.muted { t.c.muted_foreground } else { t.c.foreground.alpha_mul(0.8) }, t.c.foreground.alpha_mul(0.8)),
    };
    let line = row()
        .key(key)
        .w_full()
        .h(24.)
        .px(10.)
        .gap(12.)
        .center_y()
        .radius(theme::radius::LG)
        .hover(if t.dark { rune::Color::WHITE.alpha(0.08) } else { rune::Color::BLACK.alpha(0.035) })
        .label(format!("{}: {}", r.label, r.value))
        .help(r.help.join("\n"))
        .child(text(r.label).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate().w(LABEL_W));
    let value = text(r.value).font(theme::weight(theme::TEXT_XS, 500.).features("'tnum' 1")).color(value_tone).single_line();
    let Some(fill) = r.fill else {
        // Text-only rows span the bar column so they never widen the value column.
        return line.child(row().w(BAR_W + 12. + VALUE_W).justify_end().child(value));
    };
    let track = t.c.muted_foreground.alpha_mul(if t.dark { 0.2 } else { 0.3 });
    let width = if fill > 0. { (BAR_W * (fill / 100.) as f32).clamp(6., BAR_W) } else { 0. };
    let mut bar = el().w(BAR_W).h(3.).radius(theme::radius::FULL).fill(track);
    if width > 0. {
        bar = bar.child(el().absolute(0., 0.).w(width).h(3.).radius(theme::radius::FULL).fill(bar_tone));
    }
    if let Some(tick) = r.tick {
        bar = bar.child(el().absolute(BAR_W * (tick / 100.) as f32 - 0.5, -3.).w(1.).h(9.).fill(t.c.foreground.alpha_mul(0.6)));
    }
    line.child(bar).child(row().w(VALUE_W).justify_end().child(value))
}

fn section(t: &T, key: &str, rows: Vec<Row>, divided: bool) -> Option<El> {
    if rows.is_empty() {
        return None;
    }
    let mut out = col().w_full().px(8.).pt(9.).pb(10.);
    if divided {
        out = out.child(el().absolute(0., 0.).w_full().h(1.).fill(t.c.border.alpha_mul(0.65)));
    }
    Some(out.children(rows.into_iter().enumerate().map(|(i, r)| row_view(t, format!("{key}-{i}"), r))))
}

/// The "Usage limits" section, or `None` when the provider reports none.
pub(crate) fn limits(s: &App, t: &T, divided: bool) -> Option<El> {
    section(t, "details-usage", crate::chat::composer::usage_limits::detail_rows(s)?, divided)
}

/// The "Context window" section, or `None` before the thread reports usage.
pub(crate) fn context(s: &App, t: &T, divided: bool) -> Option<El> {
    section(t, "details-context", vec![crate::chat::composer::context::detail_row(s)?], divided)
}
