//! The interface and code font families (web `applyAppearanceFontVariables`). Rune shapes only
//! the fonts an app registers at launch, so the chosen families' system files load then, and
//! the sans and mono slots switch to them at run time once they are loaded.
use crate::App;
use crate::theme::{self, AlphaMul, T};
use rune::{El, col, el, row, text};
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

/// Whether a family's files loaded at launch.
fn loaded(name: &str) -> bool {
    LAUNCHED.get().is_some_and(|l| l.iter().any(|(n, ok)| *ok && *n == name.to_lowercase()))
}

/// `Family::Named` wants a static name; each distinct family leaks once.
fn leak(name: &str) -> &'static str {
    static NAMES: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    let mut names = NAMES.lock().unwrap_or_else(|e| e.into_inner());
    let found = names.iter().copied().find(|n| *n == name);
    found.unwrap_or_else(|| {
        let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
        names.push(leaked);
        leaked
    })
}

/// The terminal's Rune family for these client settings.
pub fn terminal_family(values: &Map<String, Value>) -> rune::Family {
    match terminal_font(values, loaded) {
        TerminalFont::Code => rune::Family::Mono,
        TerminalFont::Default => rune::Family::SystemMono,
        TerminalFont::Named(name) => rune::Family::Named(leak(&name)),
    }
}

/// The first family of `--font-composer` the app can draw: a generic monospace is the system's,
/// any other generic and the `var(--font-sans)` fallback the sans slot.
fn prompt_family(field: &str, installed: impl Fn(&str) -> bool) -> rune::Family {
    for entry in field.split(',').map(|f| f.trim().trim_matches(['"', '\'']).trim()).filter(|f| !f.is_empty()) {
        match entry.to_ascii_lowercase().as_str() {
            "monospace" | "ui-monospace" => return rune::Family::SystemMono,
            generic if GENERIC.contains(&generic) => return rune::Family::Sans,
            _ if installed(entry) => return rune::Family::Named(leak(entry)),
            _ => {}
        }
    }
    rune::Family::Sans
}

/// The prompt editor's type (`--font-composer`, `--font-size-prompt`): under separate fonts its
/// own family and px size, else the interface's `text-sm`; `leading-relaxed` either way.
pub fn prompt_style(values: &Map<String, Value>) -> rune::tok::TextStyle {
    let sm = crate::theme::TEXT_SM.lh(22.75);
    if values.get("typographyAdvanced").and_then(Value::as_bool) != Some(true) {
        return sm;
    }
    let px = size(values, "fontSizePrompt", (12.0, 20.0, 14.0)) / interface_scale(values);
    rune::tok::TextStyle { family: prompt_family(field(values, "fontFamilyComposer"), loaded), size: px, ..sm }.lh(px * 1.625)
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
    let fields = ["fontFamilySans", "fontFamilyCode", "fontFamilyTerminal", "fontFamilyComposer"];
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
    follow_smoothing(&s.client_settings.values, tx);
}

/// Web's macOS-only `-webkit-font-smoothing: antialiased`, which Rune's `Off` mode matches.
fn follow_smoothing(values: &Map<String, Value>, tx: &mut rune::Tx) {
    use std::sync::atomic::{AtomicU8, Ordering};
    static APPLIED: AtomicU8 = AtomicU8::new(0);
    if !cfg!(target_os = "macos") {
        return;
    }
    let thin = values.get("fontSmoothing").or_else(|| super::client::defaults().get("fontSmoothing")).and_then(Value::as_bool) != Some(false);
    let want = if thin { 2 } else { 1 };
    if APPLIED.swap(want, Ordering::Relaxed) != want {
        tx.set_font_smoothing(if thin { rune::FontSmoothing::Off } else { rune::FontSmoothing::On });
    }
}

const PROMPT_PREVIEW: &str = "Use $frontend-design to fix the flaky test in surface.test.ts and align the header with SettingsPanels.tsx before shipping.";
const DIFF_PREVIEW: [(char, &str); 4] = [
    (' ', "export function formatUser(user: User) {"),
    ('-', "  return user.name.toUpperCase();"),
    ('+', "  return `${user.name} <${user.email}>`; // 0O 1lI"),
    (' ', "}"),
];

/// Web's `SettingsFontPreviews`, as static samples under the font rows. Rune's composer draws
/// in the interface font, so the prompt sample sits under that row.
pub fn preview(s: &App, t: &T, key: &str) -> Option<El> {
    let values = &s.client_settings.values;
    let advanced = values.get("typographyAdvanced").and_then(Value::as_bool) == Some(true);
    match key {
        "fontFamilySans" => Some(prompt_preview(t)),
        "fontFamilyCode" if advanced => Some(code_preview(t)),
        "fontFamilyCode" => Some(col().w_full().child(code_preview(t)).child(terminal_preview(values, t))),
        "fontFamilyTerminal" => Some(terminal_preview(values, t)),
        _ => None,
    }
}

fn prompt_preview(t: &T) -> El {
    col().w_full().mt(4.0).mb(8.0).px(12.0).py(8.0).radius(8.0).border(1.0, t.c.border).fill(t.c.background).child(
        text(PROMPT_PREVIEW).font(theme::TEXT_SM.lh(20.0)).color(t.c.foreground).wrap(),
    )
}

fn code_preview(t: &T) -> El {
    let font = theme::mono(rune::TextStyle::new(t.code, 400.0).lh((t.code * 1.6).round()));
    let mut lines = col().w_full().py(4.0);
    for (sign, code) in DIFF_PREVIEW {
        let tint = match sign {
            '+' => Some(t.c.diff_addition),
            '-' => Some(t.c.diff_deletion),
            _ => None,
        };
        let mut line = row()
            .w_full()
            .px(12.0)
            .gap(12.0)
            .child(text(sign.to_string()).font(font).color(tint.unwrap_or(t.c.code_foreground)).w(8.0))
            .child(text(code).font(font).color(t.c.code_foreground));
        if let Some(tint) = tint {
            line = line.fill(tint.alpha_mul(0.18));
        }
        lines = lines.child(line);
    }
    col()
        .w_full()
        .mt(4.0)
        .mb(8.0)
        .radius(8.0)
        .clip_rounded(8.0)
        .border(1.0, t.c.border)
        .fill(t.c.code_background)
        .child(row().w_full().h(32.0).px(12.0).center_y().child(text("src/formatUser.ts").font(theme::XS_MEDIUM).color(t.c.code_foreground)))
        .child(el().w_full().h(1.0).fill(t.c.border))
        .child(lines)
}

/// The terminal sample in the terminal's own family and size, colored like its ANSI palette.
fn terminal_preview(values: &Map<String, Value>, t: &T) -> El {
    let size = terminal_size(values);
    let font = rune::TextStyle::new(size, 400.0).family(terminal_family(values)).lh((size * 1.35).round().max(1.0));
    let bold = theme::weight(font, 700.0);
    let ansi = |i: usize| {
        let [r, g, b] = if t.dark { crate::panel::vt::DARK_ANSI[i] } else { crate::panel::vt::LIGHT_ANSI[i] };
        rune::color::hex(&format!("#{r:02x}{g:02x}{b:02x}")).unwrap_or(t.c.terminal_foreground)
    };
    let (fg, dim) = (t.c.terminal_foreground, t.c.terminal_foreground.alpha_mul(0.6));
    let prompt = [("\u{2192} ", ansi(2), bold), ("t3code ", ansi(6), bold), ("git:(", ansi(4), bold), ("main", ansi(1), bold), (") ", ansi(4), bold), ("\u{2717} ", ansi(3), bold)];
    let mut first = prompt.to_vec();
    first.push(("vpr dev", fg, font));
    let lines: Vec<Vec<(&str, rune::Color, rune::TextStyle)>> = vec![
        first,
        vec![],
        vec![("  VITE", ansi(2), bold), (" v7.1.1", ansi(2), font), ("  ready in ", dim, font), ("1.24s", fg, bold)],
        vec![],
        vec![("  \u{2192}  ", ansi(2), font), ("Local:    ", dim, font), ("http://127.0.0.1:5173/", ansi(6), font)],
        vec![("  \u{2192}  ", ansi(2), font), ("Network:  ", dim, font), ("http://192.168.1.24:5173/", ansi(6), font)],
        vec![],
        vec![("  \u{2713} 85 passed", ansi(2), font), ("   \u{25b3} 2 warnings", ansi(3), font), ("   \u{2717} 0 failed", ansi(1), font)],
        vec![],
        vec![("  ", fg, font), (" READY ", ansi(0), font), (" watching for changes, press ", dim, font), ("q", fg, bold), (" to quit", dim, font)],
        vec![],
        prompt.to_vec(),
    ];
    let mut out = col().w_full().mt(4.0).mb(8.0).padding(12.0).radius(8.0).fill(t.c.terminal_background);
    for spans in lines {
        let mut line = row().w_full().h(font.line_height);
        for (span, color, style) in spans {
            let piece = text(span).font(style).color(color);
            line = line.child(if span == " READY " { el().fill(ansi(2)).child(piece) } else { piece });
        }
        out = out.child(line);
    }
    out.label("Terminal font preview")
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

pub(super) fn font_dirs() -> Vec<PathBuf> {
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

    #[test]
    fn the_prompt_font_applies_only_under_separate_fonts() {
        let values = |v: Value| v.as_object().unwrap().clone();
        let prompt = json!({"fontFamilyComposer": "monospace", "fontSizePrompt": 18});
        assert_eq!(prompt_style(&values(prompt.clone())), crate::theme::TEXT_SM.lh(22.75));
        let mut advanced = prompt;
        advanced["typographyAdvanced"] = json!(true);
        let style = prompt_style(&values(advanced.clone()));
        assert_eq!((style.family, style.size, style.line_height), (rune::Family::SystemMono, 18.0, 29.25));
        advanced["fontSizeInterface"] = json!(20);
        advanced["fontSizePrompt"] = json!(40);
        assert_eq!(prompt_style(&values(advanced)).size, 16.0, "clamped to 20px, in layout points");
        let installed = |name: &str| name == "Iosevka";
        assert_eq!(prompt_family("Missing, 'Iosevka', serif", installed), rune::Family::Named("Iosevka"));
        assert_eq!(prompt_family("Missing", installed), rune::Family::Sans);
        assert_eq!(prompt_family("system-ui, Iosevka", installed), rune::Family::Sans);
    }
}
