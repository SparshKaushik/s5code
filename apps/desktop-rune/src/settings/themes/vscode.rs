//! Web's `vscodeThemeImport.ts`: a VS Code color theme becomes a theme, its palette derived the
//! way `parseVsCodeThemeFile` derives it, and sibling files pair into light and dark variants.
use super::super::vivid;
use serde_json::{Map, Value, json};

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rgba {
    r: f64,
    g: f64,
    b: f64,
    a: f64,
}

const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
const NOT_JSON: &str = "That theme file is not valid JSON.";

/// `jsonc-parser` with trailing commas allowed: comments and trailing commas go, anything else
/// malformed is an error rather than parser recovery.
pub(crate) fn jsonc(source: &str) -> Result<Value, String> {
    let mut stripped = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            stripped.push(c);
            if c == '\\' {
                stripped.extend(chars.next());
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                stripped.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        stripped.push('\n');
                        break;
                    }
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = ' ';
                loop {
                    match chars.next() {
                        Some('/') if prev == '*' => break,
                        Some(n) => prev = n,
                        None => return Err(NOT_JSON.into()),
                    }
                }
                stripped.push(' ');
            }
            _ => stripped.push(c),
        }
    }
    let chars: Vec<char> = stripped.chars().collect();
    let mut cleaned = String::with_capacity(stripped.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if in_string {
            cleaned.push(c);
            if c == '\\' && i < chars.len() {
                cleaned.push(chars[i]);
                i += 1;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        }
        if c == ',' && matches!(chars[i..].iter().find(|n| !n.is_whitespace()), Some('}' | ']')) {
            continue;
        }
        cleaned.push(c);
    }
    serde_json::from_str(&cleaned).map_err(|_| NOT_JSON.into())
}

fn decode_gamma(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn encode_gamma(v: f64) -> f64 {
    let c = v.clamp(0.0, 1.0);
    if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn number(part: &str) -> Option<f64> {
    let v = match part.strip_suffix('%') {
        Some(p) => p.parse::<f64>().ok()? / 100.0,
        None => part.parse::<f64>().ok()?,
    };
    v.is_finite().then_some(v)
}

/// `color(srgb …)` and `color(display-p3 …)`, the latter brought into sRGB.
fn color_function(value: &str) -> Option<Rgba> {
    let lower = value.to_ascii_lowercase();
    let body = lower.strip_prefix("color(")?.strip_suffix(')')?.trim_start();
    let (space, rest) = body.split_once(char::is_whitespace)?;
    if !matches!(space, "srgb" | "display-p3") || rest.contains(')') {
        return None;
    }
    let mut halves = rest.split('/');
    let channels: Vec<f64> = halves.next()?.split_whitespace().map(number).collect::<Option<_>>()?;
    let [r, g, b] = channels[..] else { return None };
    let a = match halves.next() {
        Some(alpha) => number(alpha.trim())?,
        None => 1.0,
    }
    .clamp(0.0, 1.0);
    if space == "srgb" {
        return Some(Rgba { r: r * 255.0, g: g * 255.0, b: b * 255.0, a });
    }
    let (lr, lg, lb) = (decode_gamma(r), decode_gamma(g), decode_gamma(b));
    Some(Rgba {
        r: encode_gamma(1.2249401762805 * lr - 0.2249401762805 * lg) * 255.0,
        g: encode_gamma(-0.042056961239 * lr + 1.042056961239 * lg) * 255.0,
        b: encode_gamma(-0.0196375547643 * lr - 0.0786360655012 * lg + 1.0982736202656 * lb) * 255.0,
        a,
    })
}

/// `parseVsCodeColor`: hex with or without `#`, 3, 4, 6 or 8 digits, or `color()`.
fn parse_color(value: &Value) -> Option<Rgba> {
    let trimmed = value.as_str()?.trim();
    if trimmed.starts_with("color(") {
        return color_function(trimmed);
    }
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if !matches!(hex.len(), 3 | 4 | 6 | 8) || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let d: Vec<f64> = if hex.len() <= 4 {
        hex.chars().filter_map(|c| c.to_digit(16)).map(|v| f64::from(v * 17)).collect()
    } else {
        (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).map(f64::from).collect()
    };
    Some(Rgba { r: d[0], g: d[1], b: d[2], a: d.get(3).map_or(1.0, |a| a / 255.0) })
}

fn to_hex(c: Rgba) -> String {
    let ch = |v: f64| v.round().clamp(0.0, 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}", ch(c.r), ch(c.g), ch(c.b))
}

/// Composites a translucent VS Code overlay onto its surface; theme roles are opaque.
fn flatten_over(c: Rgba, base: Rgba) -> String {
    if c.a >= 1.0 {
        return to_hex(c);
    }
    let over = |x: f64, y: f64| x * c.a + y * (1.0 - c.a);
    to_hex(Rgba { r: over(c.r, base.r), g: over(c.g, base.g), b: over(c.b, base.b), a: 1.0 })
}

fn luminance(c: Rgba) -> f64 {
    let ch = |v: f64| {
        let n = v / 255.0;
        if n <= 0.03928 { n / 12.92 } else { ((n + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * ch(c.r) + 0.7152 * ch(c.g) + 0.0722 * ch(c.b)
}

fn contrast(a: Rgba, b: Rgba) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// `hexToRgb`: any theme color (hex or the generator's oklch) as 8-bit sRGB.
fn rgb(css: &str) -> Rgba {
    parse_color(&json!(vivid::hex(css).unwrap_or_else(|| css.to_owned()))).unwrap_or(BLACK)
}

pub(crate) fn is_theme(value: &Value) -> bool {
    let Some(object) = value.as_object() else { return false };
    if value["version"] == 1 {
        return false;
    }
    object.get("colors").and_then(Value::as_object).is_some_and(|c| c.keys().any(|k| k.contains('.'))) || value["tokenColors"].is_array()
}

/// `humanizeThemeName`: `pierre-dark-soft` reads “Pierre Dark Soft”; names with spaces stay.
pub(crate) fn humanize(raw: &str) -> String {
    let trimmed = raw.trim();
    let separators: &[char] = &['-', '_', '.'];
    if trimmed.chars().any(char::is_whitespace) || !trimmed.contains(separators) {
        return trimmed.to_owned();
    }
    trimmed
        .split(separators)
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            chars.next().map(|f| f.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// `themeIdFromName`: a slug, or for names without ASCII letters or digits an FNV-1a hash.
pub(crate) fn theme_id(name: &str) -> String {
    let label = name.trim().to_lowercase();
    let mut slug = String::new();
    for c in label.chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(48).collect();
    if !slug.is_empty() || label.is_empty() {
        return if slug.is_empty() { "custom-theme".into() } else { slug };
    }
    let mut hash: u32 = 2_166_136_261;
    for unit in label.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(16_777_619);
    }
    format!("theme-{}", radix36(hash))
}

fn radix36(mut n: u32) -> String {
    let mut digits = Vec::new();
    loop {
        digits.push(std::char::from_digit(n % 36, 36).unwrap_or('0'));
        n /= 36;
        if n == 0 {
            break;
        }
    }
    digits.iter().rev().collect()
}

fn reserved(id: &str) -> bool {
    super::RESERVED.contains(&id)
}

fn resolve_name(value: &Value) -> String {
    [&value["displayName"], &value["name"]]
        .into_iter()
        .filter_map(Value::as_str)
        .map(humanize)
        .find(|h| !h.is_empty())
        .map(|h| h.chars().take(48).collect())
        .unwrap_or_else(|| "VS Code theme".into())
}

/// `parseVsCodeThemeFile`: `stock(dark)` is the standard palette of an appearance.
pub(crate) fn convert(value: &Value, stock: impl Fn(bool) -> Value) -> Result<Value, String> {
    if !value.is_object() {
        return Err("Theme files must contain a JSON object.".into());
    }
    let empty = Map::new();
    let colors = value["colors"].as_object().unwrap_or(&empty);
    let pick = |keys: &[&str]| keys.iter().find_map(|k| colors.get(*k).and_then(parse_color));
    let solid_over = |base: Rgba, keys: &[&str]| pick(keys).map(|c| flatten_over(c, base));
    let canvas = pick(&["editor.background", "editorPane.background"])
        .ok_or(r#"That VS Code theme has no "editor.background" color, so there is nothing to build a palette from."#)?;
    let canvas = Rgba { a: 1.0, ..canvas };
    let dark = match value["type"].as_str().map(str::to_lowercase).as_deref() {
        Some("light" | "hc-light") => false,
        Some("dark" | "hc-black") => true,
        _ => luminance(canvas) < 0.179,
    };
    let standard = stock(dark);
    let canvas_hex = to_hex(canvas);
    let raised = solid_over(canvas, &["editorWidget.background", "dropdown.background"]);
    let apart = |a: &str, b: &str| contrast(rgb(a), rgb(b)) >= 1.1;
    let clear = |c: &str| apart(c, &canvas_hex) && raised.as_deref().is_none_or(|r| apart(c, r));

    let accent = ["focusBorder", "button.background", "textLink.foreground", "activityBarBadge.background", "progressBar.background", "badge.background"]
        .into_iter()
        .filter_map(|key| colors.get(key).and_then(parse_color))
        .map(|c| (c, flatten_over(c, canvas)))
        .find(|(_, hex)| clear(hex));
    let (accent_color, accent_hex) = accent.unwrap_or_else(|| {
        let standard_accent = vivid::hex(standard["accent"].as_str().unwrap_or("")).unwrap_or_else(|| "#000000".into());
        let hex = [standard_accent.as_str(), "#ffffff", "#000000"].into_iter().find(|c| clear(c)).unwrap_or(standard_accent.as_str()).to_owned();
        (parse_color(&json!(hex)).unwrap_or(BLACK), hex)
    });

    let muted_accent = flatten_over(Rgba { a: 0.2, ..accent_color }, canvas);
    let derived = vivid::colors(&standard, dark, &canvas_hex, &muted_accent);
    let d = |role: &str| derived[role].as_str().unwrap_or("").to_owned();
    let sidebar_hex = solid_over(canvas, &["sideBar.background", "activityBar.background"]).unwrap_or_else(|| d("sidebar"));
    let sidebar = rgb(&sidebar_hex);
    let terminal_hex = solid_over(canvas, &["terminal.background", "panel.background"]).unwrap_or_else(|| d("terminalBackground"));
    let terminal = rgb(&terminal_hex);
    let readable_on = |surface: &str, fallback: &str, keys: &[&str]| -> String {
        let surface = rgb(surface);
        let readable = |c: &str| contrast(rgb(c), surface) >= 4.5;
        if let Some(specified) = solid_over(surface, keys).filter(|c| readable(c)) {
            return specified;
        }
        if readable(fallback) {
            return fallback.to_owned();
        }
        (if luminance(surface) < 0.179 { "#ffffff" } else { "#000000" }).to_owned()
    };
    // Status colors also fill destructive controls under white text, so a dim one is lightened
    // along its hue; one hidden in the canvas has no hue to keep and the derived one stands in.
    let readable_status = |fallback: &str, keys: &[&str]| -> String {
        let base = solid_over(canvas, keys).filter(|c| apart(c, &canvas_hex)).unwrap_or_else(|| fallback.to_owned());
        vivid::readable_on(&base, &canvas_hex, 4.5)
    };
    let surface_raised = raised.clone().unwrap_or_else(|| d("surfaceRaised"));
    // Checked tracks use messageAction; resolve it before the unchecked input role.
    let action_hex = solid_over(canvas, &["button.background"])
        .filter(|b| apart(b, &canvas_hex) && apart(b, &surface_raised))
        .unwrap_or_else(|| accent_hex.clone());
    let distinct = |c: &str| apart(c, &canvas_hex) && apart(c, &action_hex);
    let candidates = [d("input"), d("surfaceRaised"), standard["input"].as_str().unwrap_or("").to_owned(), "#000000".into(), "#ffffff".into(), "#808080".into()];
    let input_hex = ["input.background", "input.border"]
        .into_iter()
        .find_map(|key| solid_over(canvas, &[key]).filter(|c| distinct(c)))
        .or_else(|| candidates.into_iter().find(|c| distinct(c)))
        .unwrap_or_else(|| "#808080".into());
    let search = solid_over(canvas, &["editor.findMatchHighlightBackground"]).unwrap_or_else(|| d("searchMatchBackground"));
    let search_active = solid_over(canvas, &["editor.findMatchBackground"]).unwrap_or_else(|| d("searchMatchActiveBackground"));
    let or = |found: Option<String>, role: &str| found.unwrap_or_else(|| d(role));

    let overrides: Vec<(&str, String)> = vec![
        ("canvas", canvas_hex.clone()),
        ("text", readable_on(&canvas_hex, &d("text"), &["editor.foreground", "foreground"])),
        ("textMuted", readable_on(&canvas_hex, &d("textMuted"), &["descriptionForeground", "disabledForeground"])),
        ("surface", or(solid_over(canvas, &["editorWidget.background"]), "surface")),
        ("surfaceRaised", surface_raised.clone()),
        ("surfaceOverlay", or(solid_over(canvas, &["menu.background", "quickInput.background", "dropdown.background"]), "surfaceOverlay")),
        ("border", or(solid_over(canvas, &["panel.border", "editorGroup.border", "contrastBorder"]), "border")),
        ("input", input_hex),
        ("placeholder", readable_on(&surface_raised, &d("placeholder"), &["input.placeholderForeground"])),
        ("error", readable_status(&d("error"), &["editorError.foreground", "errorForeground"])),
        ("warning", readable_status(&d("warning"), &["editorWarning.foreground"])),
        ("accentSurface", or(solid_over(canvas, &["list.activeSelectionBackground", "list.hoverBackground"]), "accentSurface")),
        ("codeBackground", or(solid_over(canvas, &["textCodeBlock.background"]), "codeBackground")),
        ("searchMatchBackground", search.clone()),
        (
            "searchMatchForeground",
            readable_on(&search, &d("searchMatchForeground"), &["editor.findMatchHighlightForeground", "editor.foreground", "foreground"]),
        ),
        ("searchMatchActiveBackground", search_active.clone()),
        (
            "searchMatchActiveForeground",
            readable_on(&search_active, &d("searchMatchActiveForeground"), &["editor.findMatchForeground", "editor.foreground", "foreground"]),
        ),
        ("sidebar", sidebar_hex.clone()),
        ("sidebarForeground", readable_on(&sidebar_hex, &d("sidebarForeground"), &["sideBar.foreground"])),
        ("sidebarBorder", or(solid_over(sidebar, &["sideBar.border"]), "sidebarBorder")),
        ("sidebarRowHover", or(solid_over(sidebar, &["list.hoverBackground"]), "sidebarRowHover")),
        ("sidebarRowActive", or(solid_over(sidebar, &["list.inactiveSelectionBackground", "list.hoverBackground"]), "sidebarRowActive")),
        ("sidebarRowSelected", or(solid_over(sidebar, &["list.activeSelectionBackground"]), "sidebarRowSelected")),
        ("terminalBackground", terminal_hex.clone()),
        ("terminalForeground", readable_on(&terminal_hex, &d("terminalForeground"), &["terminal.foreground"])),
        ("terminalCursor", or(solid_over(terminal, &["terminalCursor.foreground", "editorCursor.foreground"]), "terminalCursor")),
        ("terminalSelection", or(solid_over(terminal, &["terminal.selectionBackground", "editor.selectionBackground"]), "terminalSelection")),
        ("terminalScrollbar", or(solid_over(terminal, &["scrollbarSlider.background"]), "terminalScrollbar")),
        ("accent", accent_hex.clone()),
        ("focus", accent_hex.clone()),
        ("messageAction", action_hex.clone()),
        ("messageActionForeground", readable_on(&action_hex, &d("messageActionForeground"), &["button.foreground"])),
        ("accentForeground", readable_on(&accent_hex, &d("accentForeground"), &["button.foreground"])),
    ];
    let mut palette = derived.clone();
    for (role, css) in overrides {
        palette[role] = Value::String(css);
    }
    let name = resolve_name(value);
    let id = theme_id(&name);
    let id = if reserved(&id) { format!("{id}-vscode") } else { id };
    Ok(json!({"id": id, "label": name, "appearance": if dark { "dark" } else { "light" }, "colors": palette}))
}

/// `parseThemeFile` under a new name: the id follows the name; None when it can't be used.
fn renamed(theme: &Value, name: &str) -> Option<Value> {
    let label: String = name.trim().chars().take(48).collect();
    let id = theme_id(&label);
    if label.is_empty() || reserved(&id) {
        return None;
    }
    let mut out = json!({"id": id, "label": label, "appearance": theme["appearance"], "colors": theme["colors"]});
    for key in ["variants", "managed"] {
        if !theme[key].is_null() {
            out[key] = theme[key].clone();
        }
    }
    Some(out)
}

/// `/\b(?:light|dark)\b/gi` replaced by a space, then whitespace collapsed.
fn strip_appearance(label: &str) -> String {
    let chars: Vec<char> = label.chars().collect();
    let word = |i: usize| chars.get(i).is_some_and(|c| c.is_ascii_alphanumeric() || *c == '_');
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let hit = ["light", "dark"].into_iter().find(|w| {
            let end = i + w.len();
            end <= chars.len()
                && chars[i..end].iter().collect::<String>().eq_ignore_ascii_case(w)
                && (i == 0 || !word(i - 1))
                && !word(end)
        });
        match hit {
            Some(w) => {
                out.push(' ');
                i += w.len();
            }
            None => {
                out.push(chars[i]);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `pairVsCodeThemes`: a family's one light and one dark file become one theme with both.
pub(crate) fn pair(themes: Vec<Value>, paired_id: Option<&dyn Fn(&Value, &Value) -> String>) -> Vec<Value> {
    let mut passthrough: Vec<(usize, Value)> = Vec::new();
    let mut groups: Vec<(String, usize, Vec<Value>, Vec<Value>)> = Vec::new();
    for (order, theme) in themes.into_iter().enumerate() {
        let label = theme["label"].as_str().unwrap_or("").to_owned();
        let key = strip_appearance(&label);
        if super::modes(&theme).len() != 1 || key == label || !key.chars().any(|c| c.is_ascii_alphanumeric()) {
            passthrough.push((order, theme));
            continue;
        }
        let index = match groups.iter().position(|g| g.0 == key) {
            Some(index) => index,
            None => {
                groups.push((key, order, vec![], vec![]));
                groups.len() - 1
            }
        };
        if theme["appearance"] == "dark" { groups[index].3.push(theme) } else { groups[index].2.push(theme) }
    }
    let mut paired = Vec::new();
    for (key, order, light, dark) in groups {
        if let ([l], [d]) = (light.as_slice(), dark.as_slice()) {
            let id = paired_id.map_or_else(|| theme_id(&key), |f| f(l, d));
            if !reserved(&id) {
                let both = json!({"id": id, "label": key, "appearance": "light", "colors": l["colors"], "variants": {"dark": d["colors"]}});
                paired.push((order, both));
                continue;
            }
        }
        paired.extend(light.into_iter().chain(dark).map(|t| (order, t)));
    }
    passthrough.extend(paired);
    passthrough.sort_by_key(|(order, _)| *order);
    passthrough.into_iter().map(|(_, t)| t).collect()
}

/// `resolveThemeLabelCollisions`: same-id themes take their file's name, then a number.
pub(crate) fn resolve_collisions(entries: Vec<(Value, Option<String>)>) -> Vec<Value> {
    let count = |id: &Value| entries.iter().filter(|(t, _)| t["id"] == *id).count();
    let relabelled: Vec<Value> = entries
        .iter()
        .map(|(theme, source)| {
            if count(&theme["id"]) < 2 {
                return theme.clone();
            }
            let label = theme["label"].as_str().unwrap_or("");
            let stem = source.as_deref().map(|s| s.rsplit_once('.').map_or(s, |(stem, _)| stem));
            stem.map(humanize)
                .filter(|from_file| from_file.to_lowercase() != label.to_lowercase())
                .and_then(|from_file| renamed(theme, &from_file))
                .unwrap_or_else(|| theme.clone())
        })
        .collect();
    let mut seen: Vec<Value> = Vec::new();
    relabelled
        .into_iter()
        .map(|theme| {
            if !seen.contains(&theme["id"]) {
                seen.push(theme["id"].clone());
                return theme;
            }
            let label = theme["label"].as_str().unwrap_or("").to_owned();
            for suffix in 2..100 {
                if let Some(candidate) = renamed(&theme, &format!("{label} {suffix}"))
                    && !seen.contains(&candidate["id"])
                {
                    seen.push(candidate["id"].clone());
                    return candidate;
                }
            }
            theme
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(dark: bool) -> Value {
        super::super::palette(&super::super::builtins()[0], dark).clone()
    }

    fn hex(theme: &Value, role: &str) -> String {
        vivid::hex(theme["colors"][role].as_str().unwrap()).unwrap()
    }

    fn pierre() -> Value {
        json!({
            "name": "pierre-dark-soft",
            "type": "dark",
            "colors": {
                "editor.background": "#171717", "editor.foreground": "#d4d4d4", "foreground": "#d4d4d4",
                "sideBar.background": "#101010", "sideBar.foreground": "#8a8a8a", "sideBar.border": "#1d1d1d",
                "focusBorder": "#69b1ff", "button.background": "#69b1ff", "button.foreground": "#171717",
                "input.border": "#2c2c2c", "input.placeholderForeground": "#525252",
                "terminal.background": "#101010", "terminal.foreground": "#8a8a8a",
                "list.hoverBackground": "#1f3e5e59", "list.activeSelectionBackground": "#1f3e5e99"
            },
            "tokenColors": []
        })
    }

    #[test]
    fn a_vscode_theme_carries_its_editor_surfaces_and_accent() {
        let theme = convert(&pierre(), stock).unwrap();
        assert_eq!((theme["label"].as_str(), theme["appearance"].as_str(), theme["id"].as_str()), (Some("Pierre Dark Soft"), Some("dark"), Some("pierre-dark-soft")));
        for (role, want) in [("canvas", "#171717"), ("text", "#d4d4d4"), ("accent", "#69b1ff"), ("sidebar", "#101010"), ("terminalBackground", "#101010")] {
            assert_eq!(hex(&theme, role), want, "FAILED: {role}");
        }
        // Translucent list colors composite onto the canvas.
        assert_eq!(hex(&theme, "accentSurface"), flatten_over(parse_color(&json!("#1f3e5e99")).unwrap(), parse_color(&json!("#171717")).unwrap()));
        assert!(stock(true).as_object().unwrap().keys().all(|role| theme["colors"].get(role).is_some()), "every role is filled");
        assert!(is_theme(&pierre()));
        assert!(is_theme(&json!({"type": "dark", "tokenColors": []})));
        assert!(!is_theme(&json!({"version": 1, "name": "Aurora", "appearance": "light", "colors": {"canvas": "#fff"}})));
        assert!(convert(&json!({"colors": {"editor.foreground": "#fff"}}), stock).unwrap_err().contains("editor.background"));
    }

    #[test]
    fn vscode_conversion_keeps_text_readable_and_names_reserved_ids_apart() {
        let theme = convert(
            &json!({"name": "Dark placeholder fallback", "type": "dark", "colors": {"editor.background": "#1e1e2e", "editor.foreground": "#cdd6f4", "input.placeholderForeground": "#cdd6f473"}}),
            stock,
        )
        .unwrap();
        let c = |role: &str| rgb(&hex(&theme, role));
        assert!(contrast(c("placeholder"), c("surfaceRaised")) >= 4.5);
        assert!(contrast(c("placeholder"), c("canvas")) < contrast(c("text"), c("canvas")));
        let dark_plus = convert(&json!({"name": "Dark+", "type": "dark", "colors": {"editor.background": "#1e1e1e"}}), stock).unwrap();
        assert_eq!((dark_plus["label"].as_str(), dark_plus["id"].as_str()), (Some("Dark+"), Some("dark-vscode")));
        // A light canvas with no type reads as light; wide-gamut notation parses.
        let light = convert(&json!({"name": "x", "colors": {"editor.background": "color(srgb 1 1 1)"}}), stock).unwrap();
        assert_eq!((light["appearance"].as_str(), hex(&light, "canvas").as_str()), (Some("light"), "#ffffff"));
        assert!(parse_color(&json!("color(display-p3 1 0 0 / 50%)")).is_some_and(|c| c.a == 0.5 && c.r > 254.0));
        assert_eq!(parse_color(&json!("#abc")), Some(Rgba { r: 170.0, g: 187.0, b: 204.0, a: 1.0 }));
        assert_eq!(parse_color(&json!("#12345")), None);
    }

    #[test]
    fn jsonc_drops_comments_and_trailing_commas_but_not_malformed_input() {
        for source in [
            "{ // line\n \"name\": \"Test\", \"colors\": { \"editor.background\": \"#002b36\" } }",
            "{ /* block */ \"name\": \"Test\", \"colors\": { \"editor.background\": \"#002b36\" } }",
            "{ \"name\": \"Test\", \"colors\": { \"editor.background\": \"#002b36\", }, \"tokenColors\": [ { \"scope\": [\"comment\",], }, ], }",
        ] {
            assert_eq!(jsonc(source).unwrap()["colors"]["editor.background"], "#002b36", "FAILED: {source}");
        }
        assert_eq!(jsonc(r#"{"url": "http://x//y", "s": "a,}"}"#).unwrap()["s"], "a,}", "strings are left alone");
        for bad in ["", "{ \"name\": \"Test\"", "{ \"a\": 1 } /* open", "{ \"a\": 1 } extra"] {
            assert!(jsonc(bad).is_err(), "FAILED: {bad}");
        }
    }

    #[test]
    fn families_pair_and_same_names_take_their_file_names() {
        let theme = |label: &str, dark: bool| {
            json!({"id": theme_id(label), "label": label, "appearance": if dark { "dark" } else { "light" }, "colors": {"canvas": if dark { "#000000" } else { "#ffffff" }}})
        };
        let paired = pair(vec![theme("Solar Light", false), theme("Other", true), theme("Solar Dark", true)], None);
        assert_eq!(paired.len(), 2);
        assert_eq!((paired[0]["label"].as_str(), paired[0]["variants"]["dark"]["canvas"].as_str()), (Some("Solar"), Some("#000000")));
        assert_eq!(paired[1]["label"], "Other");
        let plus = pair(vec![theme("Light+", false), theme("Dark+", true)], None);
        assert_eq!(plus.iter().map(|t| t["label"].as_str().unwrap()).collect::<Vec<_>>(), ["Light+", "Dark+"], "no pair named +");
        let ambiguous = pair(vec![theme("A Light", false), theme("A Light", false), theme("A Dark", true)], None);
        assert_eq!(ambiguous.len(), 3);
        let resolved = resolve_collisions(vec![(theme("Nord", true), Some("nord-frost.json".into())), (theme("Nord", true), Some("nord.json".into())), (theme("Nord", true), None)]);
        assert_eq!(resolved.iter().map(|t| t["label"].as_str().unwrap()).collect::<Vec<_>>(), ["Nord Frost", "Nord", "Nord 2"]);
        assert_eq!(humanize("pierre-dark-soft"), "Pierre Dark Soft");
        assert_eq!(humanize("Tokyo Night"), "Tokyo Night");
        assert_eq!((theme_id(" Tokyo  Night! "), theme_id(""), theme_id("日本")), ("tokyo-night".into(), "custom-theme".into(), format!("theme-{}", radix36({
            let mut h: u32 = 2_166_136_261;
            for u in "日本".encode_utf16() { h ^= u32::from(u); h = h.wrapping_mul(16_777_619); }
            h
        }))));
    }
}
