//! The interface and code font families (web `applyAppearanceFontVariables`). Rune shapes only
//! the fonts an app registers at launch, so the chosen families' system files load then, and
//! the sans and mono slots switch to them at run time once they are loaded.
use crate::App;
use serde_json::{Map, Value};
use std::path::PathBuf;
use std::sync::OnceLock;

const GENERIC: &[&str] = &[
    "serif", "sans-serif", "monospace", "cursive", "fantasy", "system-ui", "ui-sans-serif", "ui-serif", "ui-monospace", "ui-rounded", "emoji", "math",
];

const INTER: [&[u8]; 3] = [
    include_bytes!("../../assets/fonts/Inter-Regular.ttf"),
    include_bytes!("../../assets/fonts/Inter-Medium.ttf"),
    include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"),
];

/// Families named at launch: (lowercase name, whether its files were found).
static LAUNCHED: OnceLock<Vec<(String, bool)>> = OnceLock::new();

/// The named (non-generic) families in a comma-separated field, unquoted.
fn named(field: &str) -> Vec<String> {
    field
        .split(',')
        .map(|f| f.trim().trim_matches(['"', '\'']).trim())
        .filter(|f| !f.is_empty() && !GENERIC.contains(&f.to_ascii_lowercase().as_str()))
        .map(str::to_owned)
        .collect()
}

/// A field as a CSS family list ending in `fallback`, named families quoted.
fn css(field: &str, fallback: &str) -> String {
    let entries = field.split(',').map(|f| f.trim().trim_matches(['"', '\'']).trim()).filter(|f| !f.is_empty());
    let quoted = entries.map(|f| if GENERIC.contains(&f.to_ascii_lowercase().as_str()) { f.to_owned() } else { format!("\"{}\"", f.replace('"', "")) });
    quoted.chain([fallback.to_owned()]).collect::<Vec<_>>().join(", ")
}

fn field<'a>(values: &'a Map<String, Value>, key: &str) -> &'a str {
    values.get(key).and_then(Value::as_str).unwrap_or("")
}

fn size(values: &Map<String, Value>, key: &str, (min, max, default): (f64, f64, f64)) -> f32 {
    let px = values.get(key).and_then(Value::as_f64).filter(|v| v.is_finite()).unwrap_or(default);
    px.round().clamp(min, max) as f32
}

/// The size that zooms the whole window (`desktop::frame`): a change lands in one commit
/// (`tx.instant()`), as View › Zoom does, instead of springing every element to its new layout.
pub const ZOOM_KEY: &str = "fontSizeInterface";

/// Web's root font size over its 16px default: the factor rem-sized UI draws at.
pub fn interface_scale(values: &Map<String, Value>) -> f32 {
    size(values, ZOOM_KEY, (12.0, 20.0, 16.0)) / 16.0
}

/// `--font-size-code` in layout points: px in web, so the interface scale doesn't grow it.
pub fn code_size(values: &Map<String, Value>) -> f32 {
    size(values, "fontSizeCode", (10.0, 18.0, 13.0)) / interface_scale(values)
}

/// Web's `resolveTerminalFontSizePreference`, in layout points like [`code_size`].
pub fn terminal_size(values: &Map<String, Value>) -> f32 {
    if values.get("typographyAdvanced").and_then(Value::as_bool) == Some(true) {
        size(values, "fontSizeTerminal", (8.0, 20.0, 12.0)) / interface_scale(values)
    } else {
        code_size(values)
    }
}

/// Which font the terminal draws in (`resolveTerminalFontPreference`, then ghostty's
/// `terminalFontFamily`): the code font, or under advanced typography the terminal's own first
/// installed family, else the default terminal stack.
#[derive(Debug, PartialEq)]
enum TerminalFont {
    Code,
    Default,
    Named(String),
}

fn terminal_font(values: &Map<String, Value>, installed: impl Fn(&str) -> bool) -> TerminalFont {
    if values.get("typographyAdvanced").and_then(Value::as_bool) != Some(true) {
        return TerminalFont::Code;
    }
    named(field(values, "fontFamilyTerminal")).into_iter().find(|name| installed(name)).map_or(TerminalFont::Default, TerminalFont::Named)
}

/// The terminal's Rune family for these client settings.
pub fn terminal_family(values: &Map<String, Value>) -> rune::Family {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let loaded = |name: &str| LAUNCHED.get().is_some_and(|l| l.iter().any(|(n, ok)| *ok && *n == name.to_lowercase()));
    match terminal_font(values, loaded) {
        TerminalFont::Code => rune::Family::Mono,
        TerminalFont::Default => rune::Family::SystemMono,
        TerminalFont::Named(name) => {
            let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
            // `Family::Named` wants a static name; each distinct family leaks once.
            let found = names.iter().copied().find(|n| *n == name);
            rune::Family::Named(found.unwrap_or_else(|| {
                let leaked: &'static str = Box::leak(name.into_boxed_str());
                names.push(leaked);
                leaked
            }))
        }
    }
}

/// The sans and mono slots for these client settings.
pub fn families(values: &Map<String, Value>) -> (String, String) {
    (css(field(values, "fontFamilySans"), crate::theme::SANS), css(field(values, "fontFamilyCode"), "ui-monospace"))
}

/// What a family field's row tells the user when one of its families can't show yet.
pub fn pending(field: &str) -> Option<String> {
    let launched = LAUNCHED.get()?;
    named(field).into_iter().find_map(|name| match launched.iter().find(|(n, _)| *n == name.to_lowercase()) {
        Some((_, true)) => None,
        Some((_, false)) => Some(format!("{name} isn't installed on this computer.")),
        None => Some(format!("Restart S5 Code to load {name}.")),
    })
}

/// `AppSpec::fonts` and `families` for the saved client settings: the chosen families' system
/// files with Inter behind them (registering any font replaces Rune's built-in Inter).
pub fn launch() -> (&'static [&'static [u8]], (&'static str, &'static str)) {
    let values = super::client::ClientSettings::load().values;
    let (sans, mono) = families(&values);
    let mut fonts: Vec<&'static [u8]> = Vec::new();
    let mut launched: Vec<(String, bool)> = Vec::new();
    let fields = ["fontFamilySans", "fontFamilyCode", "fontFamilyTerminal"];
    for name in fields.into_iter().flat_map(|key| named(field(&values, key))) {
        if launched.iter().any(|(n, _)| *n == name.to_lowercase()) {
            continue;
        }
        let before = fonts.len();
        for path in files(&name) {
            if let Ok(bytes) = std::fs::read(&path) {
                fonts.push(Box::leak(bytes.into_boxed_slice()));
            }
        }
        launched.push((name.to_lowercase(), fonts.len() > before));
    }
    let _ = LAUNCHED.set(launched);
    if !fonts.is_empty() {
        fonts.extend(INTER);
    }
    let leak = |s: String| -> &'static str { Box::leak(s.into_boxed_str()) };
    (Box::leak(fonts.into_boxed_slice()), (leak(sans), leak(mono)))
}

/// Points the slots at the current settings' families; free when they already hold them.
pub fn follow(s: &mut App, tx: &mut rune::Tx) {
    let want = families(&s.client_settings.values);
    if s.ui.settings.fonts_applied.as_ref() != Some(&want) {
        tx.set_font_families(want.0.clone(), want.1.clone());
        s.ui.settings.fonts_applied = Some(want);
    }
}

fn squash(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The font files of an installed family: fontconfig's exact match on Linux, else the files in
/// the platform's font folders whose names start with the family's.
fn files(family: &str) -> Vec<PathBuf> {
    if cfg!(target_os = "linux") {
        let pattern: String = family.chars().flat_map(|c| if matches!(c, '\\' | '-' | ':' | ',') { vec!['\\', c] } else { vec![c] }).collect();
        if let Ok(out) = std::process::Command::new("fc-list").arg("--format=%{file}\n").arg(format!(":family={pattern}")).output() {
            let found: Vec<PathBuf> = String::from_utf8_lossy(&out.stdout).lines().filter(|l| !l.is_empty()).map(PathBuf::from).take(16).collect();
            if !found.is_empty() {
                return found;
            }
        }
    }
    let key = squash(family);
    let mut found = Vec::new();
    let mut stack: Vec<(PathBuf, u8)> = font_dirs().into_iter().map(|d| (d, 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            let font = path.extension().and_then(|e| e.to_str()).is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "ttf" | "otf" | "ttc"));
            if font && path.file_stem().and_then(|s| s.to_str()).is_some_and(|stem| squash(stem).starts_with(&key)) {
                found.push(path);
            }
        }
    }
    found.sort();
    found.truncate(16);
    found
}

fn font_dirs() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    if cfg!(target_os = "macos") {
        vec!["/System/Library/Fonts".into(), "/Library/Fonts".into(), home.join("Library/Fonts")]
    } else if cfg!(windows) {
        let root = std::env::var_os("WINDIR").map_or_else(|| PathBuf::from("C:\\Windows"), PathBuf::from);
        let mut dirs = vec![root.join("Fonts")];
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft\\Windows\\Fonts"));
        }
        dirs
    } else {
        vec!["/usr/share/fonts".into(), "/usr/local/share/fonts".into(), home.join(".local/share/fonts"), home.join(".fonts")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn family_fields_become_slot_lists_behind_the_defaults() {
        let values = |sans: &str, code: &str| json!({"fontFamilySans": sans, "fontFamilyCode": code}).as_object().unwrap().clone();
        assert_eq!(families(&values("", "")), (crate::theme::SANS.to_owned(), "ui-monospace".to_owned()));
        assert_eq!(
            families(&values(" Georgia , 'Times New Roman', serif", "JetBrains Mono")),
            (format!("\"Georgia\", \"Times New Roman\", serif, {}", crate::theme::SANS), "\"JetBrains Mono\", ui-monospace".to_owned()),
        );
        assert_eq!(named("\"Fira Code\", monospace, ,"), ["Fira Code"]);
        assert_eq!(squash("Segoe UI-Bold"), "segoeuibold");
        let advanced = json!({"typographyAdvanced": true, "fontFamilyTerminal": "Not Loaded Mono"}).as_object().unwrap().clone();
        assert_eq!(terminal_family(&advanced), rune::Family::SystemMono, "a family that never loaded falls back");
    }

    #[test]
    fn terminal_font_follows_code_unless_advanced_names_an_installed_family() {
        let values = |v: Value| v.as_object().unwrap().clone();
        let installed = |name: &str| name == "Iosevka";
        assert_eq!(terminal_font(&values(json!({"fontFamilyTerminal": "Iosevka"})), installed), TerminalFont::Code);
        let advanced = |family: &str| values(json!({"typographyAdvanced": true, "fontFamilyTerminal": family}));
        assert_eq!(terminal_font(&advanced("Missing Mono, Iosevka, monospace"), installed), TerminalFont::Named("Iosevka".into()));
        assert_eq!(terminal_font(&advanced(""), installed), TerminalFont::Default);
        assert_eq!(terminal_font(&advanced("Missing Mono"), installed), TerminalFont::Default);
    }

    #[test]
    fn sizes_follow_web_clamps_and_the_advanced_terminal_override() {
        let values = |v: Value| v.as_object().unwrap().clone();
        let defaults = values(json!({}));
        assert_eq!((interface_scale(&defaults), code_size(&defaults), terminal_size(&defaults)), (1.0, 13.0, 13.0));
        let big = values(json!({"fontSizeInterface": 20, "fontSizeCode": 15, "fontSizeTerminal": 11}));
        assert_eq!((interface_scale(&big), code_size(&big), terminal_size(&big)), (1.25, 12.0, 12.0));
        let advanced = values(json!({"fontSizeInterface": 99, "fontSizeCode": 2, "fontSizeTerminal": 10, "typographyAdvanced": true}));
        assert_eq!((interface_scale(&advanced), code_size(&advanced), terminal_size(&advanced)), (1.25, 8.0, 8.0));
    }
}
