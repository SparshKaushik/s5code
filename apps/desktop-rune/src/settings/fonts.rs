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
    let mut launched = Vec::new();
    for name in named(field(&values, "fontFamilySans")).into_iter().chain(named(field(&values, "fontFamilyCode"))) {
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
    }
}
