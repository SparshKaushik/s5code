use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ClientSettings {
    pub values: Map<String, Value>,
    path: PathBuf,
    pub error: Option<String>,
}

pub fn defaults() -> &'static Map<String, Value> {
    static DEFAULTS: std::sync::OnceLock<Map<String, Value>> = std::sync::OnceLock::new();
    DEFAULTS.get_or_init(|| serde_json::from_str(include_str!("defaults.json")).unwrap())
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            values: defaults().clone(),
            path: config_path(),
            error: None,
        }
    }
}

/// Keys only Rune reads. Electron's settings struct drops unknown keys when it saves the
/// shared file, so these live beside it.
const RUNE_ONLY: [&str; 5] = ["appearance", "customThemes", "themeDark", "themeLight", "typographyAdvanced"];
const RUNE_FILE: &str = "desktop-rune-client-settings.json";

/// Electron's `decodeJsonResult` leniency: comments and trailing commas, outside strings.
pub(crate) fn lenient_json(text: &str) -> Option<Value> {
    serde_json::from_str(text).ok().or_else(|| serde_json::from_str(&strip_trailing_commas(&strip_comments(text))).ok())
}

/// Walks `text` outside strings, letting `skip` drop characters there.
fn outside_strings(text: &str, mut skip: impl FnMut(&[char], usize) -> usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (mut out, mut i, mut string) = (String::with_capacity(text.len()), 0, false);
    while i < chars.len() {
        let c = chars[i];
        if string {
            out.push(c);
            if c == '\\' && i + 1 < chars.len() {
                out.push(chars[i + 1]);
                i += 1;
            } else if c == '"' {
                string = false;
            }
            i += 1;
            continue;
        }
        let skipped = skip(&chars, i);
        if skipped > 0 {
            i += skipped;
            continue;
        }
        string = c == '"';
        out.push(c);
        i += 1;
    }
    out
}

fn strip_comments(text: &str) -> String {
    outside_strings(text, |chars, i| match (chars[i], chars.get(i + 1)) {
        ('/', Some('/')) => chars[i..].iter().position(|&c| c == '\n').unwrap_or(chars.len() - i),
        ('/', Some('*')) => chars[i + 2..].windows(2).position(|w| w == ['*', '/']).map_or(chars.len() - i, |end| end + 4),
        _ => 0,
    })
}

fn strip_trailing_commas(text: &str) -> String {
    outside_strings(text, |chars, i| {
        let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
        usize::from(chars[i] == ',' && matches!(next, Some('}' | ']')))
    })
}

/// Electron's `readClientSettings` shape: a legacy `{ "settings": … }` wrapper, and
/// `usageModelAliases` renamed to `usageModelTags`.
fn normalize(mut values: Map<String, Value>) -> Map<String, Value> {
    if values.len() == 1
        && let Some(Value::Object(inner)) = values.get("settings")
    {
        values = inner.clone();
    }
    if let Some(aliases) = values.remove("usageModelAliases") {
        values.entry("usageModelTags").or_insert(aliases);
    }
    values
}

fn read(path: &Path) -> Result<Option<Map<String, Value>>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => match lenient_json(&text) {
            Some(Value::Object(values)) => Ok(Some(normalize(values))),
            _ => Err(format!("Could not read client settings: {} is not a settings object", path.display())),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read client settings: {error}")),
    }
}

/// Electron's `writeClientSettings`: through a symlink to its target, by a temporary file.
fn write(path: &Path, values: &Map<String, Value>) -> Result<(), Box<dyn std::error::Error>> {
    let target = if std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) { std::fs::canonicalize(path)? } else { path.to_owned() };
    if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let temporary = target.with_file_name(format!("{name}.{}.{}.tmp", std::process::id(), s5_net::uuid().replace('-', "")));
    let mut text = serde_json::to_string_pretty(values)?;
    text.push('\n');
    let written = std::fs::write(&temporary, text).and_then(|_| std::fs::rename(&temporary, &target));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    Ok(written?)
}

impl ClientSettings {
    /// The shared file in the T3 home, else the one Rune kept before (moved on the next save),
    /// with Rune's own keys from beside it.
    pub fn load() -> Self {
        let mut settings = Self::default();
        let shared = match read(&settings.path) {
            Ok(None) if std::env::var_os("S5_CLIENT_SETTINGS").is_none() => read(&legacy_path()).unwrap_or_default(),
            other => other.unwrap_or_else(|e| {
                settings.error = Some(e);
                None
            }),
        };
        settings.values.extend(shared.unwrap_or_default());
        if let Ok(Some(own)) = read(&settings.path.with_file_name(RUNE_FILE)) {
            settings.values.extend(own.into_iter().filter(|(k, _)| RUNE_ONLY.contains(&k.as_str())));
        }
        settings
    }

    /// A file beside the settings for client UI state (the web's other localStorage keys).
    pub fn sibling(&self, name: &str) -> PathBuf {
        sibling_path(name)
    }

    pub fn save(&mut self) {
        // Preserve an unreadable file so a later setting change cannot destroy it.
        if self.error.is_some() {
            return;
        }
        let (own, shared): (Map<String, Value>, Map<String, Value>) = self.values.clone().into_iter().partition(|(k, _)| RUNE_ONLY.contains(&k.as_str()));
        let result = write(&self.path, &shared).and_then(|_| write(&self.path.with_file_name(RUNE_FILE), &own));
        if let Err(error) = result {
            self.error = Some(format!("Could not save client settings: {error}"));
        }
    }
}

/// A file kept beside Rune's own client state, such as the composer drafts.
pub fn sibling_path(name: &str) -> PathBuf {
    match std::env::var_os("S5_CLIENT_SETTINGS") {
        Some(path) => PathBuf::from(path).with_file_name(name),
        None => legacy_path().with_file_name(name),
    }
}

/// Electron's `<stateDir>/client-settings.json`, so both apps share one set.
pub(crate) fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("S5_CLIENT_SETTINGS") {
        return path.into();
    }
    crate::host::t3_home().join("userdata").join("client-settings.json")
}

/// Where Rune kept client settings before, and still keeps its other client state.
fn legacy_path() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let base = if cfg!(target_os = "macos") {
        home.join("Library/Application Support")
    } else if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or(home.join("AppData/Roaming"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or(home.join(".config"))
    };
    base.join("S5 Code/client-settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_preserves_unknown_settings_and_replaces_the_complete_file() {
        let path = std::env::temp_dir().join(format!("s5-settings-{}.json", std::process::id()));
        let mut settings = ClientSettings {
            path: path.clone(),
            ..Default::default()
        };
        settings
            .values
            .insert("futureSetting".into(), serde_json::json!({"nested":true}));
        settings
            .values
            .insert("appearance".into(), serde_json::json!("dark"));
        settings.save();
        assert!(settings.error.is_none());
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["futureSetting"]["nested"], true);
        assert!(saved.get("appearance").is_none(), "Electron would drop Rune's own keys");
        let own = path.with_file_name(RUNE_FILE);
        let saved: Value = serde_json::from_slice(&std::fs::read(&own).unwrap()).unwrap();
        assert_eq!(saved["appearance"], "dark");
        settings
            .values
            .insert("appearance".into(), serde_json::json!("light"));
        settings.save();
        let saved: Value = serde_json::from_slice(&std::fs::read(&own).unwrap()).unwrap();
        assert_eq!(saved["appearance"], "light");
        std::fs::remove_file(path).unwrap();
        std::fs::remove_file(own).unwrap();
    }

    #[test]
    fn reads_electrons_file_leniently() {
        let text = "{\n  // written by hand\n  \"settings\": { \"usageModelAliases\": {\"a\": \"b\"}, \"url\": \"http://x/*y*/,\", /* c */ \"n\": [1, 2,], },\n}";
        let Some(Value::Object(values)) = lenient_json(text) else { panic!("lenient JSON") };
        let values = normalize(values);
        assert_eq!(Value::Object(values), serde_json::json!({"usageModelTags": {"a": "b"}, "url": "http://x/*y*/,", "n": [1, 2]}));
        assert_eq!(lenient_json("{\"a\": 1"), None);
    }

    #[test]
    fn an_unreadable_file_is_never_overwritten() {
        let path =
            std::env::temp_dir().join(format!("s5-settings-invalid-{}.json", std::process::id()));
        std::fs::write(&path, b"invalid json").unwrap();
        let mut settings = ClientSettings {
            path: path.clone(),
            error: Some("Could not read client settings".into()),
            ..Default::default()
        };
        settings.save();
        assert_eq!(std::fs::read(&path).unwrap(), b"invalid json");
        std::fs::remove_file(path).unwrap();
    }
}
