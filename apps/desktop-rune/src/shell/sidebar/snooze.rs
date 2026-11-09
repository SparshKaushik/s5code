//! `CustomSnoozeDialog`: snooze until a local date and time, or for a duration. The menu's
//! "Custom…" opens it for the row or the multi-selection; Snooze applies to every target.

use rune::{El, TextEdit, Tx, col, el, icon, row, text};

use crate::data::Zone;
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons, kit};

const DAY: i64 = 86_400_000;
const UNITS: [(&str, i64); 3] = [("Minutes", 60_000), ("Hours", 3_600_000), ("Days", DAY)];

pub struct Custom {
    pub targets: Vec<String>,
    pub duration: bool,
    pub date: TextEdit,
    pub time: TextEdit,
    pub amount: TextEdit,
    pub unit: usize,
    pub unit_open: bool,
    pub calendar_open: bool,
    /// The month the calendar shows, as (year, month).
    pub month: (i64, i64),
    pub error: Option<&'static str>,
}

impl Custom {
    /// Opens on an hour from now, as the web's `initial`.
    pub fn new(targets: Vec<String>, now: i64, zone: Zone) -> Custom {
        let (date, time) = local_parts(now + 3_600_000, zone);
        let (y, m, _) = parse_date(&date).unwrap_or((1970, 1, 1));
        let (date, mut time) = (TextEdit::new(date), TextEdit::new(time));
        time.focused = true;
        Custom { targets, duration: false, date, time, amount: TextEdit::new("2"), unit: 1, unit_open: false, calendar_open: false, month: (y, m), error: None }
    }
}

/// Days since the epoch for a proleptic Gregorian date (Howard Hinnant).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `localSnoozeDate` and `localSnoozeTime`: "2026-11-08" and "14:05".
fn local_parts(at: i64, zone: Zone) -> (String, String) {
    let wall = zone.wall(at);
    let (y, m, d) = civil_from_days(wall.div_euclid(DAY));
    let mins = wall.rem_euclid(DAY) / 60_000;
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}", mins / 60, mins % 60))
}

fn digits(s: &str, len: usize) -> Option<i64> {
    (s.len() == len && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

fn parse_date(s: &str) -> Option<(i64, i64, i64)> {
    let mut parts = s.trim().split('-');
    let date = (digits(parts.next()?, 4)?, digits(parts.next()?, 2)?, digits(parts.next()?, 2)?);
    parts.next().is_none().then_some(date)
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// The trigger's `toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" })`.
fn date_label((y, m, d): (i64, i64, i64)) -> String {
    format!("{} {d}, {y}", &MONTHS[(m - 1).clamp(0, 11) as usize][..3])
}

fn add_months((y, m): (i64, i64), by: i64) -> (i64, i64) {
    let n = y * 12 + (m - 1) + by;
    (n.div_euclid(12), n.rem_euclid(12) + 1)
}

/// DayPicker's month: whole Sunday-first weeks covering the month, outside days included.
fn month_grid((y, m): (i64, i64)) -> Vec<(i64, i64, i64)> {
    let first = days_from_civil(y, m, 1);
    let (ny, nm) = add_months((y, m), 1);
    let len = days_from_civil(ny, nm, 1) - first;
    // The epoch was a Thursday.
    let lead = (first + 4).rem_euclid(7);
    let weeks = (lead + len + 6) / 7;
    (0..weeks * 7).map(|i| civil_from_days(first - lead + i)).collect()
}

/// `resolveCustomSnooze`: the wake instant, or `None` for an invalid or past choice.
pub fn resolve(c: &Custom, now: i64, zone: Zone) -> Option<i64> {
    let wake = if c.duration {
        let amount: f64 = c.amount.text.trim().parse().ok()?;
        if !amount.is_finite() || amount <= 0.0 {
            return None;
        }
        now + (amount * UNITS[c.unit].1 as f64).round() as i64
    } else {
        let (y, m, d) = parse_date(&c.date.text)?;
        let mut time = c.time.text.trim().split(':');
        let (h, min) = (digits(time.next()?, 2)?, digits(time.next()?, 2)?);
        if time.next().is_some() || h > 23 || min > 59 {
            return None;
        }
        let wall = days_from_civil(y, m, d) * DAY + h * 3_600_000 + min * 60_000;
        let at = zone.instant(wall);
        // Rejects rolled-over dates and local times a DST change skips.
        if local_parts(at, zone) != (c.date.text.trim().to_string(), c.time.text.trim().to_string()) {
            return None;
        }
        at
    };
    (wake > now).then_some(wake)
}

fn submit(s: &mut App) {
    let Some(c) = s.ui.shell.sidebar.custom_snooze.as_ref() else { return };
    let Some(until) = resolve(c, crate::data::now_ms(), Zone::Local) else {
        let error = if c.duration { "Enter a positive duration." } else { "Choose a valid date and time in the future." };
        if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
            c.error = Some(error);
        }
        return;
    };
    let targets = s.ui.shell.sidebar.custom_snooze.take().map(|c| c.targets).unwrap_or_default();
    super::snooze_many(s, &targets, until);
}

fn close(s: &mut App) {
    s.ui.shell.sidebar.custom_snooze = None;
}

fn edit(s: &mut App, f: fn(&mut Custom) -> &mut TextEdit) -> &mut TextEdit {
    let c = s.ui.shell.sidebar.custom_snooze.as_mut().expect("the dialog is open while it edits");
    c.error = None;
    f(c)
}

fn field(label: &str, body: El, t: &T) -> El {
    col().grow(1.0).min_w(0.0).gap(8.0).child(text(label.to_string()).font(theme::SM_MEDIUM).color(t.c.contrast_foreground)).child(body)
}

pub fn dialog(s: &App, t: &T, overlays: &mut Vec<El>) {
    let Some(c) = &s.ui.shell.sidebar.custom_snooze else { return };
    let k = kit::theme(t);
    // Popups draw over the dialog, so they join the layer after it.
    let mut popups = Vec::new();
    let mode = kit::ToggleGroup::new(&["Date and time", "Duration"], &[usize::from(c.duration)])
        .on_pick(|s: &mut App, i, _: &mut Tx| {
            if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
                c.duration = i == 1;
                c.error = None;
            }
        })
        .build(&k)
        .w_full();
    let inputs = if c.duration {
        let amount = kit::Input::new(&c.amount)
            .placeholder("2")
            .on_edit(|s: &mut App| edit(s, |c| &mut c.amount))
            .on_submit(|s: &mut App, _| submit(s))
            .build(t);
        let unit = kit::Select::new("sb-snooze-unit", &UNITS.map(|u| u.0))
            .current(Some(UNITS[c.unit].0))
            .label("Unit")
            .width(162.0)
            .open(c.unit_open)
            .on_toggle(|s: &mut App| {
                if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
                    c.unit_open ^= true;
                }
            })
            .on_close(|s: &mut App| {
                if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
                    c.unit_open = false;
                }
            })
            .on_choose(|s: &mut App, label: &str, _: &mut Tx| {
                if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
                    c.unit = UNITS.iter().position(|u| u.0 == label).unwrap_or(1);
                    c.unit_open = false;
                    c.error = None;
                }
            })
            .build(&k, &mut popups);
        row().w_full().gap(12.0).child(field("Snooze for", amount, t)).child(field("Unit", unit, t))
    } else {
        let date = date_picker(c, t, &k, &mut popups);
        let time = kit::Input::new(&c.time)
            .placeholder("HH:MM")
            .on_edit(|s: &mut App| edit(s, |c| &mut c.time))
            .on_submit(|s: &mut App, _| submit(s))
            .build(t);
        row().w_full().gap(12.0).child(field("Date", date, t)).child(field("Time", time, t))
    };
    let mut body = col().w_full().gap(16.0).child(mode).child(inputs);
    if let Some(error) = c.error {
        body = body.child(text(error).font(theme::TEXT_SM).color(t.c.destructive).role("alert"));
    }
    let options = kit::Button::new("Edit snooze options")
        .variant("ghost-muted")
        .size("sm")
        .icon(&icons::SETTINGS_2)
        .on_press(|s: &mut App| {
            close(s);
            super::go(s, Route::Settings("threads".into()));
        })
        .build(t);
    let cancel = kit::Button::new("Cancel").variant("outline").size("sm").on_press(close).build(t);
    let confirm = kit::Button::new("Snooze").size("sm").on_press(submit).build(t);
    kit::Dialog::new("sb-custom-snooze", "Custom snooze")
        .open(true)
        .width(384.0)
        .description("Choose when snoozed threads return to your inbox.")
        .child(body)
        .child(row().w_full().gap(8.0).center_y().child(options).child(rune::spacer()).child(cancel).child(confirm))
        .on_close(close)
        .build(t, overlays);
    overlays.extend(popups);
}

fn with_custom(s: &mut App, f: impl FnOnce(&mut Custom)) {
    if let Some(c) = s.ui.shell.sidebar.custom_snooze.as_mut() {
        f(c);
    }
}

fn date_picker(c: &Custom, t: &T, k: &rune_kit::Kit, popups: &mut Vec<El>) -> El {
    let selected = parse_date(&c.date.text);
    let label = selected.map(date_label).unwrap_or_else(|| "Pick a date".into());
    let trigger = row()
        .key("sb-snooze-date")
        .role("button")
        .label("Date")
        .w(162.0)
        .h(30.0)
        .px(11.0)
        .gap(8.0)
        .center_y()
        .radius(8.0)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .border(1.0, t.c.contrast_input)
        .hover(t.c.accent)
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.contrast_foreground))
        .child(rune::spacer())
        .child(icon(&icons::CALENDAR, 16.0).color(t.c.muted_foreground))
        .on_press(|s: &mut App| {
            with_custom(s, |c| {
                c.calendar_open ^= true;
                if let Some((y, m, _)) = parse_date(&c.date.text) {
                    c.month = (y, m);
                }
            })
        });
    let today = parse_date(&local_parts(crate::data::now_ms(), Zone::Local).0).unwrap_or((1970, 1, 1));
    kit::Popover::new("sb-snooze-calendar")
        .label("Choose snooze date")
        .placement(kit::Placement::BottomStart)
        .padding(12.0)
        .trigger(trigger)
        .child(calendar(c.month, selected, today, t))
        .open(c.calendar_open)
        .on_close(|s: &mut App| with_custom(s, |c| c.calendar_open = false))
        .build(k, popups)
}

const CELL: f32 = 36.0;

fn calendar(month: (i64, i64), selected: Option<(i64, i64, i64)>, today: (i64, i64, i64), t: &T) -> El {
    let nav = |key: &str, glyph: &'static rune::Icon, label: &str, by: i64| {
        el().key(key.to_string())
            .role("button")
            .label(label.to_string())
            .size(CELL, CELL)
            .radius(8.0)
            .center()
            .hover(t.c.accent)
            .child(icon(glyph, 16.0).color(t.c.foreground.alpha_mul(0.8)))
            .on_press(move |s: &mut App| with_custom(s, |c| c.month = add_months(c.month, by)))
    };
    let caption = format!("{} {}", MONTHS[(month.1 - 1) as usize], month.0);
    let header = row()
        .w_full()
        .center_y()
        .child(nav("sb-cal-prev", &icons::CHEVRON_LEFT, "Go to the Previous Month", -1))
        .child(row().grow(1.0).justify_center().child(text(caption).font(theme::SM_MEDIUM).color(t.c.foreground)))
        .child(nav("sb-cal-next", &icons::CHEVRON_RIGHT, "Go to the Next Month", 1));
    let muted = t.c.muted_foreground.alpha_mul(0.72);
    let weekdays = row().children(["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"].map(|d| {
        el().size(CELL, CELL).center().child(text(d).font(theme::XS_MEDIUM).color(muted))
    }));
    let mut grid = col().child(weekdays);
    for week in month_grid(month).chunks(7) {
        grid = grid.child(row().children(week.iter().map(|&day| {
            let (y, m, d) = day;
            let is_selected = selected == Some(day);
            let disabled = day < today;
            let outside = m != month.1;
            let label = d.to_string();
            let color = if is_selected {
                t.c.primary_foreground
            } else if disabled || outside {
                muted
            } else {
                t.c.foreground
            };
            let mut digits = text(label.clone()).font(theme::TEXT_SM).color(color);
            if disabled {
                digits = digits.strike(0, label.len(), None);
            }
            let mut cell = el().key(format!("sb-cal-{y}-{m}-{d}")).size(CELL, CELL).radius(8.0).center().child(digits);
            if is_selected {
                cell = cell.fill(t.c.primary);
            }
            if day == today {
                let dot = if is_selected { t.c.background } else { t.c.primary };
                cell = cell.child(el().size(3.0, 3.0).radius(2.0).fill(dot).absolute((CELL - 3.0) / 2.0, CELL - 7.0));
            }
            if disabled {
                return cell.opacity(0.64);
            }
            if !is_selected {
                cell = cell.hover(t.c.accent);
            }
            cell.role("button").label(date_label(day)).on_press(move |s: &mut App| {
                with_custom(s, |c| {
                    c.date = TextEdit::new(format!("{y:04}-{m:02}-{d:02}"));
                    c.calendar_open = false;
                    c.error = None;
                })
            })
        })));
    }
    col().gap(4.0).child(header).child(grid)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custom(date: &str, time: &str) -> Custom {
        let mut c = Custom::new(vec![], 0, Zone::Fixed(0));
        c.date = TextEdit::new(date);
        c.time = TextEdit::new(time);
        c
    }

    #[test]
    fn resolves_a_local_date_and_time() {
        let now = days_from_civil(2026, 11, 8) * DAY + 12 * 3_600_000;
        let zone = Zone::Fixed(-5 * 3_600_000);
        let mut c = custom("2026-11-09", "09:30");
        assert_eq!(resolve(&c, now, zone), Some(days_from_civil(2026, 11, 9) * DAY + 14 * 3_600_000 + 30 * 60_000));
        c.date = TextEdit::new("2026-02-30");
        assert_eq!(resolve(&c, now, zone), None, "rolled-over dates are rejected");
        c.date = TextEdit::new("2026-11-08");
        c.time = TextEdit::new("06:59");
        assert_eq!(resolve(&c, now, zone), None, "the past is rejected");
        c.time = TextEdit::new("9:30");
        assert_eq!(resolve(&c, now, zone), None, "HH:MM only");
    }

    #[test]
    fn resolves_a_positive_duration() {
        let mut c = custom("", "");
        c.duration = true;
        c.amount = TextEdit::new("1.5");
        c.unit = 1;
        assert_eq!(resolve(&c, 1000, Zone::Fixed(0)), Some(1000 + 5_400_000));
        c.amount = TextEdit::new("0");
        assert_eq!(resolve(&c, 1000, Zone::Fixed(0)), None);
        c.amount = TextEdit::new("abc");
        assert_eq!(resolve(&c, 1000, Zone::Fixed(0)), None);
    }

    #[test]
    fn month_grids_hold_whole_sunday_first_weeks() {
        let nov = month_grid((2026, 11));
        assert_eq!((nov.len(), nov[0], nov[34]), (35, (2026, 11, 1), (2026, 12, 5)));
        let feb = month_grid((2026, 2));
        assert_eq!((feb.len(), feb[0], feb[27]), (28, (2026, 2, 1), (2026, 2, 28)));
        let jan = month_grid((2027, 1));
        assert_eq!(jan[0], (2026, 12, 27), "outside days lead into a Friday first");
        assert_eq!((add_months((2026, 1), -1), add_months((2026, 12), 1)), ((2025, 12), (2027, 1)));
        assert_eq!(date_label((2026, 11, 9)), "Nov 9, 2026");
    }

    #[test]
    fn opens_an_hour_ahead() {
        let now = days_from_civil(2026, 12, 31) * DAY + 23 * 3_600_000 + 15 * 60_000;
        let c = Custom::new(vec![], now, Zone::Fixed(0));
        assert_eq!((c.date.text.as_str(), c.time.text.as_str()), ("2027-01-01", "00:15"));
    }
}
