//! Electron's `getSystemLocale` where web uses it (`timestampFormat.ts`): the "locale" timestamp
//! format takes the OS region's hour cycle, and the snooze calendar its first weekday. Rune has no
//! ICU, so both come from CLDR's region preferences for the regions that differ from the default.

use std::sync::OnceLock;

/// CLDR `timeData`: regions whose preferred clock is 12-hour.
const HOUR12: &[&str] = &[
    "US", "CA", "AU", "NZ", "IN", "PH", "PK", "BD", "EG", "SA", "AE", "JO", "KW", "QA", "OM", "BH", "IQ", "LY", "SY", "YE", "LB", "SD", "MY", "SG",
    "HK", "TW", "KR", "CO", "MX", "VE", "SV", "HN", "NI", "PA", "PR", "DO", "GT", "PE", "AS", "GU", "MP", "VI", "UM", "BN", "JM", "TT", "BS",
];
/// CLDR `weekData.firstDay`: Sunday-first regions; Saturday-first ones are `SATURDAY`.
const SUNDAY: &[&str] = &[
    "AG", "AS", "BD", "BR", "BS", "BT", "BW", "BZ", "CA", "CN", "CO", "DM", "DO", "ET", "GT", "GU", "HK", "HN", "ID", "IL", "IN", "JM", "JP", "KE", "KH", "KR",
    "LA", "MH", "MM", "MO", "MT", "MX", "MZ", "NI", "NP", "PA", "PE", "PH", "PK", "PR", "PT", "PY", "SA", "SG", "SV", "TH", "TT", "TW", "UM", "US", "VE", "VI",
    "WS", "YE", "ZA", "ZW",
];
const SATURDAY: &[&str] = &["AE", "AF", "BH", "DJ", "DZ", "EG", "IQ", "IR", "JO", "KW", "LY", "OM", "QA", "SD", "SY"];

/// The OS locale as a BCP 47 tag (`en-GB`), read once.
pub fn system() -> Option<&'static str> {
    static LOCALE: OnceLock<Option<String>> = OnceLock::new();
    LOCALE.get_or_init(read).as_deref()
}

#[cfg(target_os = "macos")]
fn read() -> Option<String> {
    super::macos::locale_identifier().and_then(|id| normalize(&id))
}

/// glibc's precedence for time formatting.
#[cfg(not(target_os = "macos"))]
fn read() -> Option<String> {
    ["LC_ALL", "LC_TIME", "LANG"].iter().filter_map(|k| std::env::var(k).ok()).find(|v| !v.trim().is_empty()).and_then(|v| normalize(&v))
}

/// `en_GB.UTF-8`, `en_GB@euro` or AppKit's `en_GB@rg=gbzzzz` to `en-GB`; `C` and `POSIX` are none.
fn normalize(raw: &str) -> Option<String> {
    let tag = raw.split(['.', '@']).next()?.trim().replace('_', "-");
    if tag.is_empty() || tag == "C" || tag == "POSIX" {
        return None;
    }
    Some(tag)
}

/// The language and region of a tag, the region upper-cased (`zh-Hant-TW` -> `TW`).
fn parts(tag: &str) -> (String, Option<String>) {
    let mut it = tag.split('-');
    let language = it.next().unwrap_or_default().to_ascii_lowercase();
    let region = it.find(|p| p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic())).map(str::to_ascii_uppercase);
    (language, region)
}

/// The region a bare language most likely means, as `Intl.Locale#maximize` would for the common ones.
fn region_of(tag: &str) -> Option<String> {
    let (language, region) = parts(tag);
    const LIKELY: &[(&str, &str)] = &[
        ("en", "US"), ("de", "DE"), ("fr", "FR"), ("es", "ES"), ("it", "IT"), ("pt", "BR"), ("nl", "NL"), ("sv", "SE"), ("pl", "PL"),
        ("ru", "RU"), ("tr", "TR"), ("ja", "JP"), ("zh", "CN"), ("ko", "KR"), ("ar", "EG"), ("hi", "IN"),
    ];
    region.or_else(|| LIKELY.iter().find(|(l, _)| *l == language).map(|(_, r)| (*r).to_owned()))
}

fn locale_hour12(tag: Option<&str>) -> bool {
    let Some(tag) = tag else { return true };
    let (language, _) = parts(tag);
    match region_of(tag) {
        // Quebec French writes 24-hour time.
        Some(r) if r == "CA" && language == "fr" => false,
        Some(r) => HOUR12.contains(&r.as_str()),
        None => true,
    }
}

/// The `timestampFormat` setting: 12- or 24-hour as set, else the system locale's clock.
pub fn hour12(format: Option<&str>) -> bool {
    match format {
        Some("12-hour") => true,
        Some("24-hour") => false,
        _ => locale_hour12(system()),
    }
}

fn locale_week_start(tag: Option<&str>) -> u8 {
    match tag.and_then(region_of) {
        Some(r) if SUNDAY.contains(&r.as_str()) => 0,
        Some(r) if SATURDAY.contains(&r.as_str()) => 6,
        Some(_) => 1,
        // DayPicker's own default when the runtime has no week data.
        None => 0,
    }
}

/// Web's `weekStartsOn`: the first weekday as a `Date#getDay` index, 0 for Sunday.
pub fn week_start() -> u8 {
    locale_week_start(system())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_locale_sets_the_clock_and_the_first_weekday() {
        assert_eq!(normalize("en_GB.UTF-8").as_deref(), Some("en-GB"));
        assert_eq!(normalize("de_DE@euro").as_deref(), Some("de-DE"));
        assert_eq!(normalize("en_US@rg=gbzzzz").as_deref(), Some("en-US"));
        assert_eq!(normalize("C"), None);
        assert_eq!(normalize("POSIX"), None);
        assert_eq!(parts("zh-Hant-TW"), ("zh".into(), Some("TW".into())));

        for (tag, h12, week) in [
            (Some("en-US"), true, 0),
            (Some("en-GB"), false, 1),
            (Some("de-DE"), false, 1),
            (Some("fr-CA"), false, 0),
            (Some("en-CA"), true, 0),
            (Some("ja-JP"), false, 0),
            (Some("ar-EG"), true, 6),
            (Some("en"), true, 0),
            (Some("de"), false, 1),
            (Some("xx"), true, 0),
            (None, true, 0),
        ] {
            assert_eq!(locale_hour12(tag), h12, "{tag:?}");
            assert_eq!(locale_week_start(tag), week, "{tag:?}");
        }
        assert!(hour12(Some("12-hour")) && !hour12(Some("24-hour")));
    }
}
