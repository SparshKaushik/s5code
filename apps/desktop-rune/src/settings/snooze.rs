//! The snooze options editor (`SnoozePresetsRow`): one row per menu option with its "When"
//! select, a duration or a time, the option's menu preview, and add, remove and reset.
//!
//! Fields are settings keys under `snooze::` (`snooze::when::2`), so the page's select and
//! input plumbing edits them and `value`/`update` route here.

use rune::{El, col, row, text};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use super::{controls, server_settings, update};
use crate::theme::{self, AlphaMul, T};
use crate::data::Zone;
use crate::{App, icons};

const DAY: i64 = 86_400_000;
const MAX_PRESETS: usize = 8;
const MAX_MINUTES: i64 = 30 * 24 * 60;
const UNITS: [(&str, i64, &str); 3] = [("minutes", 1, "minute"), ("hours", 60, "hour"), ("days", 24 * 60, "day")];
const WHEN: [(&str, &str); 10] = [
    ("after", "In"),
    ("today", "Today"),
    ("tomorrow", "Tomorrow"),
    ("monday", "Monday"),
    ("tuesday", "Tuesday"),
    ("wednesday", "Wednesday"),
    ("thursday", "Thursday"),
    ("friday", "Friday"),
    ("saturday", "Saturday"),
    ("sunday", "Sunday"),
];
const WEEKDAYS: [&str; 7] = ["sunday", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday"];
/// `toLocaleDateString(undefined, { weekday: "short" })`, from the epoch's Thursday.
const SHORT_DAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];

/// The editor's draft: rows keep their order while edited, as the web sorts only on settle.
#[derive(Default)]
pub struct Draft {
    pub rows: Option<Vec<Value>>,
    /// A row's chosen unit, so stepping 59 → 60 minutes doesn't jump to hours.
    pub units: HashMap<usize, String>,
}

fn defaults() -> Vec<Value> {
    super::scope::defaults()["snoozePresets"].as_array().cloned().unwrap_or_default()
}

fn saved(s: &App) -> Vec<Value> {
    server_settings(s)["snoozePresets"].as_array().cloned().unwrap_or_else(defaults)
}

fn id(p: &Value) -> String {
    match p["kind"].as_str() {
        Some("after") => format!("after:{}", p["minutes"]),
        _ => format!("at:{}:{}", crate::data::s(p, "day"), crate::data::s(p, "time")),
    }
}

fn ids(list: &[Value]) -> Vec<String> {
    let mut ids: Vec<_> = list.iter().map(id).collect();
    ids.sort();
    ids
}

/// The rows shown: the draft while it matches what's saved, else the saved list sorted.
fn rows(s: &App) -> Vec<Value> {
    let saved = saved(s);
    match &s.ui.settings.snooze.rows {
        Some(draft) if ids(draft) == ids(&saved) => draft.clone(),
        _ => sort(saved, s.now_ms, Zone::Local),
    }
}

pub fn customized(s: &App) -> bool {
    ids(&rows(s)) != ids(&defaults())
}

pub fn reset(s: &mut App) {
    s.ui.settings.snooze.units.clear();
    let sorted = sort(defaults(), s.now_ms, Zone::Local);
    save(s, sorted);
}

fn save(s: &mut App, list: Vec<Value>) {
    s.ui.settings.snooze.rows = Some(list.clone());
    update(s, "snoozePresets", json!(list));
}

fn clock(wall: i64) -> String {
    let mins = wall.rem_euclid(DAY) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
}

fn duration(minutes: i64) -> String {
    let (n, unit) = if minutes % (24 * 60) == 0 {
        (minutes / (24 * 60), "day")
    } else if minutes % 60 == 0 {
        (minutes / 60, "hour")
    } else {
        (minutes, "minute")
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn capitalize(word: &str) -> String {
    let mut c = word.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

fn hm(time: &str) -> Option<(i64, i64)> {
    let (h, m) = time.split_once(':')?;
    if h.len() != 2 || m.len() != 2 {
        return None;
    }
    let (h, m) = (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// `resolveSnoozePreset` in `zone`: `(label, whenLabel, wake)`, or none for a "today" option
/// whose time has passed.
fn resolve(p: &Value, now: i64, zone: Zone) -> Option<(String, String, i64)> {
    let wall_now = zone.wall(now);
    let today = wall_now - wall_now.rem_euclid(DAY);
    let (label, wake, names_day) = if p["kind"] == "after" {
        let minutes = p["minutes"].as_i64()?;
        (format!("In {}", duration(minutes)), now + minutes * 60_000, false)
    } else {
        let day = crate::data::s(p, "day");
        let weekday = (wall_now.div_euclid(DAY) + 4).rem_euclid(7);
        let ahead = match day {
            "today" => 0,
            "tomorrow" => 1,
            d => {
                let target = WEEKDAYS.iter().position(|w| *w == d)? as i64;
                match (target - weekday + 7).rem_euclid(7) {
                    0 => 7,
                    n => n,
                }
            }
        };
        let (h, m) = hm(crate::data::s(p, "time"))?;
        let wake = zone.instant(today + ahead * DAY + (h * 60 + m) * 60_000);
        if wake <= now {
            return None;
        }
        let label = match ahead {
            0 if h < 12 => "This morning".to_owned(),
            0 if h < 17 => "This afternoon".to_owned(),
            0 => "This evening".to_owned(),
            1 => "Tomorrow".to_owned(),
            _ if day == "monday" => "Next week".to_owned(),
            7 => format!("Next {}", capitalize(day)),
            _ => capitalize(day),
        };
        let names_day = label != "Next week";
        (label, wake, names_day)
    };
    let wall = zone.wall(wake);
    let when = if names_day || wall.div_euclid(DAY) == wall_now.div_euclid(DAY) {
        clock(wall)
    } else {
        format!("{} {}", SHORT_DAYS[wall.div_euclid(DAY).rem_euclid(7) as usize], clock(wall))
    };
    Some((label, when, wake))
}

/// `sortSnoozeRows`: soonest wake first, passed options last, ties kept in order.
pub fn sort(mut list: Vec<Value>, now: i64, zone: Zone) -> Vec<Value> {
    list.sort_by_key(|p| resolve(p, now, zone).map_or(i64::MAX, |(_, _, w)| w));
    list
}

/// `resolveSnoozePresets`: the menu as it reads now, one entry per wake minute.
fn menu(list: &[Value], now: i64) -> Vec<(String, String)> {
    let mut entries: Vec<_> = list.iter().filter_map(|p| resolve(p, now, Zone::Local)).collect();
    entries.sort_by_key(|(_, _, w)| w / 60_000);
    let mut seen = HashSet::new();
    entries.into_iter().filter(|(_, _, w)| seen.insert(w / 60_000)).map(|(l, w, _)| (l, w)).collect()
}

fn unit_of(s: &App, index: usize, minutes: i64) -> (&'static str, i64, &'static str) {
    let chosen = s.ui.settings.snooze.units.get(&index).and_then(|u| UNITS.iter().find(|x| x.0 == u));
    match chosen {
        Some(u) if minutes % u.1 == 0 => *u,
        _ => *UNITS.iter().rev().find(|u| minutes % u.1 == 0).unwrap_or(&UNITS[0]),
    }
}

/// A `[snooze::]<field>::<row>` key's field and row; the prefix is optional so the row's own
/// controls can pass the key they render with.
fn field(key: &str) -> Option<(&str, usize)> {
    let (field, index) = key.trim_start_matches("snooze::").split_once("::")?;
    Some((field, index.parse().ok()?))
}

/// The value behind a `snooze::<field>::<row>` key.
pub fn value(s: &App, key: &str) -> Value {
    let Some((field, index)) = field(key) else { return Value::Null };
    let Some(p) = rows(s).get(index).cloned() else { return Value::Null };
    let minutes = p["minutes"].as_i64().unwrap_or(60);
    match field {
        "when" => json!(if p["kind"] == "after" { "after" } else { crate::data::s(&p, "day") }),
        "unit" => json!(unit_of(s, index, minutes).0),
        "amount" => json!(minutes / unit_of(s, index, minutes).1),
        "time" => p["time"].clone(),
        _ => Value::Null,
    }
}

pub fn set(s: &mut App, key: &str, next: Value) {
    let Some((field, index)) = field(key) else { return };
    let mut list = rows(s);
    let Some(p) = list.get(index).cloned() else { return };
    let minutes = p["minutes"].as_i64().unwrap_or(60);
    let replaced = match field {
        "when" => match next.as_str() {
            Some("after") if p["kind"] == "after" => return,
            Some("after") => json!({"kind": "after", "minutes": 60}),
            Some(day) => {
                let time = p["time"].as_str().unwrap_or("09:00");
                json!({"kind": "at", "day": day, "time": time})
            }
            None => return,
        },
        "unit" => {
            let Some(unit) = next.as_str().and_then(|u| UNITS.iter().find(|x| x.0 == u)) else { return };
            let amount = minutes / unit_of(s, index, minutes).1;
            s.ui.settings.snooze.units.insert(index, unit.0.to_owned());
            json!({"kind": "after", "minutes": (amount * unit.1).min(MAX_MINUTES)})
        }
        "amount" => {
            let (_, size, _) = unit_of(s, index, minutes);
            let Some(amount) = next.as_f64() else { return };
            let amount = (amount.round() as i64).clamp(1, MAX_MINUTES / size);
            json!({"kind": "after", "minutes": amount * size})
        }
        "time" => {
            let Some(time) = next.as_str().filter(|t| hm(t).is_some()) else { return };
            json!({"kind": "at", "day": p["day"], "time": time})
        }
        _ => return,
    };
    if replaced == p {
        return;
    }
    list[index] = replaced;
    save(s, list);
}

fn add(s: &mut App) {
    let mut list = rows(s);
    if list.len() >= MAX_PRESETS {
        return;
    }
    let used: HashSet<_> = list.iter().map(id).collect();
    let candidates = defaults().into_iter().chain([
        json!({"kind": "after", "minutes": 30}),
        json!({"kind": "at", "day": "saturday", "time": "09:00"}),
        json!({"kind": "after", "minutes": 24 * 60}),
    ]);
    let next = candidates.into_iter().find(|c| !used.contains(&id(c))).unwrap_or(json!({"kind": "after", "minutes": 60}));
    list.push(next);
    save(s, list);
}

fn remove(s: &mut App, index: usize) {
    let mut list = rows(s);
    if index < list.len() {
        list.remove(index);
        // Chosen units follow their rows up.
        let units = std::mem::take(&mut s.ui.settings.snooze.units);
        s.ui.settings.snooze.units = units
            .into_iter()
            .filter(|(i, _)| *i != index)
            .map(|(i, u)| (if i > index { i - 1 } else { i }, u))
            .collect();
        save(s, list);
    }
}

/// `NumberField` sm: −, the centred amount, +, inside one bordered w-32 group.
fn amount_field(s: &App, t: &T, index: usize) -> El {
    let key = format!("snooze::amount::{index}");
    let amount = value(s, &key).as_i64().unwrap_or(1);
    let step = |delta: i64, glyph: &'static rune::Icon, label: &str| {
        let k = key.clone();
        crate::kit::Button::new(label)
            .variant("ghost")
            .size("icon-sm")
            .icon(glyph)
            .on_press(move |s: &mut App| {
                s.ui.settings.edits.remove(&k);
                set(s, k.trim_start_matches("snooze::"), json!(amount + delta));
            })
            .build(t)
            .w(34.0)
            .h(26.0)
    };
    row()
        .w(128.0)
        .h(28.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .border(1.0, t.c.input)
        .fill(if t.dark { t.c.input.alpha_mul(0.32) } else { t.c.background })
        .clip_rounded(theme::layout::CONTROL_RADIUS)
        .child(step(-1, &icons::MINUS, "Decrease"))
        .child(controls::input(s, t, &key, 58.0, "", false).h(26.0).border(0.0, t.c.input).fill(rune::Color::TRANSPARENT).grow(1.0))
        .child(step(1, &icons::PLUS, "Increase"))
}

/// `SnoozePresetFields`: the When select, then a duration or a time.
fn fields(s: &App, t: &T, index: usize, p: &Value) -> El {
    let when: Vec<_> = WHEN.iter().map(|(v, l)| (json!(v), (*l).to_owned())).collect();
    let mut out = row().gap(8.0).center_y().child(controls::select(s, t, &format!("snooze::when::{index}"), "When", &when, 128.0));
    if p["kind"] == "after" {
        let singular = value(s, &format!("snooze::amount::{index}")).as_i64() == Some(1);
        let units: Vec<_> = UNITS.iter().map(|(v, _, one)| (json!(v), if singular { *one } else { *v }.to_owned())).collect();
        out = out
            .child(amount_field(s, t, index))
            .child(controls::select(s, t, &format!("snooze::unit::{index}"), "Unit", &units, 112.0));
    } else {
        out = out
            .child(text("at").font(theme::TEXT_XS).color(t.c.muted_foreground))
            .child(controls::input(s, t, &format!("snooze::time::{index}"), 128.0, "09:00", false));
    }
    out
}

/// The row's reset control comes from `setting_row`; this is the list under its copy.
pub fn parts(s: &App, t: &T) -> (El, Option<El>) {
    let list = rows(s);
    let now = s.now_ms;
    let mut seen_ids = HashSet::new();
    let mut seen_wakes = HashSet::new();
    let mut body = col().w_full().gap(6.0).pt(12.0).pb(8.0);
    for (index, p) in list.iter().enumerate() {
        let preview = match resolve(p, now, Zone::Local) {
            None => Err("Passed for today"),
            Some(_) if seen_ids.contains(&id(p)) => Err("Duplicate of an option above"),
            Some((_, _, wake)) if seen_wakes.contains(&wake) => Err("Same time as an option above"),
            Some((label, when, wake)) => {
                seen_ids.insert(id(p));
                seen_wakes.insert(wake);
                Ok((label, when))
            }
        };
        let preview = match preview {
            Ok((label, when)) => row()
                .min_w(0.0)
                .child(text(label).font(theme::TEXT_XS).color(t.c.foreground).truncate())
                .child(text(when).font(theme::TEXT_XS).color(t.c.muted_foreground).pl(8.0).truncate()),
            Err(copy) => row().min_w(0.0).child(text(copy).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
        };
        body = body.child(
            row()
                .key(format!("snooze-row-{index}"))
                .w_full()
                .gap(8.0)
                .center_y()
                .role("group")
                .label(format!("Snooze option {}", index + 1))
                .child(fields(s, t, index, p))
                .child(rune::spacer())
                .child(
                    row().min_w(0.0).gap(8.0).center_y().child(preview).child(
                        crate::kit::Button::new(format!("Remove snooze option {}", index + 1))
                            .variant("ghost-muted")
                            .size("icon-sm")
                            .icon(&icons::X)
                            .on_press(move |s: &mut App| remove(s, index))
                            .build(t),
                    ),
                ),
        );
    }
    let order = menu(&list, now);
    let labels: Vec<_> = order.iter().map(|(l, _)| l.clone()).collect();
    let summary: Vec<_> = order
        .iter()
        .map(|(l, w)| if labels.iter().filter(|x| *x == l).count() > 1 { format!("{l} {w}") } else { l.clone() })
        .collect();
    body = body.child(
        text(if summary.is_empty() { "Menus offer only Custom…".to_owned() } else { format!("Menu now: {}", summary.join(" · ")) })
            .font(theme::TEXT_XS)
            .color(t.c.muted_foreground)
            .wrap(),
    );
    let full = list.len() >= MAX_PRESETS;
    let mut footer = row().center_y().child(
        crate::kit::Button::new("Add option")
            .variant("ghost-muted")
            .size("compact")
            .icon(&icons::PLUS)
            .disabled(full)
            .on_press(add)
            .build(t),
    );
    if full {
        footer = footer.child(text(format!("Up to {MAX_PRESETS} options")).font(theme::TEXT_XS).color(t.c.muted_foreground).pl(8.0));
    }
    (rune::el(), Some(body.child(footer)))
}

#[cfg(test)]
mod tests {
    use super::*;
    // 2026-11-05 is a Thursday; 10:00 UTC.
    const NOW: i64 = 1_793_872_800_000;
    const UTC: Zone = Zone::Fixed(0);

    #[test]
    fn row_keys_resolve_with_or_without_the_snooze_prefix() {
        assert_eq!(field("amount::2"), Some(("amount", 2)));
        assert_eq!(field("snooze::amount::2"), Some(("amount", 2)), "the stepper and unit labels read the full key");
        assert_eq!(field("snooze::amount"), None);
    }

    #[test]
    fn labels_follow_the_day_gap() {
        assert_eq!((NOW.div_euclid(DAY) + 4).rem_euclid(7), 4);
        let at = |day: &str, time: &str| resolve(&json!({"kind":"at","day":day,"time":time}), NOW, UTC).map(|r| r.0);
        assert_eq!(at("today", "09:00"), None);
        assert_eq!(at("today", "18:00").as_deref(), Some("This evening"));
        assert_eq!(at("today", "13:00").as_deref(), Some("This afternoon"));
        assert_eq!(at("friday", "09:00").as_deref(), Some("Tomorrow"));
        assert_eq!(at("monday", "09:00").as_deref(), Some("Next week"));
        assert_eq!(at("thursday", "09:00").as_deref(), Some("Next Thursday"));
        assert_eq!(at("saturday", "09:00").as_deref(), Some("Saturday"));
        let after = resolve(&json!({"kind":"after","minutes":24 * 60}), NOW, UTC).unwrap();
        assert_eq!((after.0.as_str(), after.1.as_str()), ("In 1 day", "Fri 10:00 AM"));
    }

    #[test]
    fn times_read_in_the_local_zone() {
        // 10:00 UTC is 3:30 PM in India, so "today 13:00" has passed and "today 18:00" is
        // 12:30 UTC. In Los Angeles it is still Thursday 2:00 AM, so Friday is tomorrow.
        let ist = Zone::Fixed(330 * 60_000);
        let at = |day: &str, time: &str, zone| resolve(&json!({"kind":"at","day":day,"time":time}), NOW, zone);
        assert_eq!(at("today", "13:00", ist), None);
        let evening = at("today", "18:00", ist).unwrap();
        assert_eq!((evening.1.as_str(), evening.2), ("6:00 PM", NOW + 150 * 60_000));
        let after = resolve(&json!({"kind":"after","minutes":60}), NOW, ist).unwrap();
        assert_eq!(after.1, "4:30 PM");
        let la = Zone::Fixed(-8 * 3_600_000);
        assert_eq!(at("today", "09:00", la).unwrap().0, "This morning");
        assert_eq!(at("friday", "09:00", la).unwrap().0, "Tomorrow");
        assert_eq!(at("saturday", "09:00", la).unwrap().0, "Saturday");
    }

    #[test]
    fn sort_puts_passed_options_last() {
        let list = vec![
            json!({"kind":"at","day":"today","time":"08:00"}),
            json!({"kind":"after","minutes":180}),
            json!({"kind":"after","minutes":60}),
        ];
        let sorted = sort(list, NOW, UTC);
        assert_eq!(sorted.iter().map(id).collect::<Vec<_>>(), ["after:60", "after:180", "at:today:08:00"]);
    }
}
