use serde_json::{Map, Value};
use std::path::PathBuf;

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

impl ClientSettings {
    pub fn load() -> Self {
        let mut settings = Self::default();
        match std::fs::read(&settings.path) {
            Ok(bytes) => match serde_json::from_slice::<Map<String, Value>>(&bytes) {
                Ok(values) => settings.values.extend(values),
                Err(error) => {
                    settings.error = Some(format!("Could not read client settings: {error}"))
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => settings.error = Some(format!("Could not read client settings: {error}")),
        }
        settings
    }

    pub fn save(&mut self) {
        // Preserve an unreadable file so a later setting change cannot destroy it.
        if self.error.is_some() {
            return;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent)?;
            }
            let temporary = self
                .path
                .with_extension(format!("{}.tmp", std::process::id()));
            std::fs::write(&temporary, serde_json::to_vec_pretty(&self.values)?)?;
            std::fs::rename(&temporary, &self.path)?;
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(format!("Could not save client settings: {error}"));
        }
    }
}

fn config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("S5_CLIENT_SETTINGS") {
        return path.into();
    }
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
        assert_eq!(saved["appearance"], "dark");
        settings
            .values
            .insert("appearance".into(), serde_json::json!("light"));
        settings.save();
        let saved: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["appearance"], "light");
        std::fs::remove_file(path).unwrap();
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
