//! The managed policy (packages/shared/src/managedPolicyFile.ts), for the limits the desktop
//! applies itself, as Electron's DesktopManagedPolicy: network exposure and browser cookie
//! import. The server reads and enforces the rest. A policy that can't be read fails closed.

use std::sync::OnceLock;

use serde_json::{Map, Value};

#[derive(Debug, PartialEq)]
pub enum Policy {
    None,
    Document(Value),
    /// The path, and why the policy can't be used.
    Unreadable(String, String),
}

const DOCUMENT_KEYS: [&str; 5] = ["schemaVersion", "organization", "supportUrl", "settings", "limits"];
const LIMIT_KEYS: [&str; 8] = ["maxRuntimeMode", "clients", "connect", "webhookTasks", "telemetry", "browserCookieImport", "providers", "updates"];
#[cfg(target_os = "macos")]
const PROFILE: &str = "/Library/Managed Preferences/com.t3tools.t3code.plist";

/// `managedPolicyFilePath`.
pub fn file_path() -> &'static str {
    if cfg!(target_os = "macos") { "/Library/Application Support/T3Code/policy.json" } else { "/etc/t3code/policy.json" }
}

/// Read once, as Electron caches it.
pub fn current() -> &'static Policy {
    static POLICY: OnceLock<Policy> = OnceLock::new();
    POLICY.get_or_init(|| {
        let file = std::fs::read_to_string(file_path()).map(Some).or_else(|e| if e.kind() == std::io::ErrorKind::NotFound { Ok(None) } else { Err(e.to_string()) });
        read(file, profile())
    })
}

/// The macOS configuration profile as JSON, through `plutil` as Electron reads it.
#[cfg(target_os = "macos")]
fn profile() -> Result<Option<String>, String> {
    if !std::path::Path::new(PROFILE).exists() {
        return Ok(None);
    }
    let out = std::process::Command::new("/usr/bin/plutil").args(["-convert", "json", "-o", "-", PROFILE]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    Ok(Some(String::from_utf8_lossy(&out.stdout).into_owned()))
}

#[cfg(not(target_os = "macos"))]
fn profile() -> Result<Option<String>, String> {
    Ok(None)
}

/// `readManagedPolicy`: one source or none; both at once is an error, never a merge.
fn read(file: Result<Option<String>, String>, profile: Result<Option<String>, String>) -> Policy {
    let unreadable = |path: &str, why: String| Policy::Unreadable(path.to_owned(), why);
    #[cfg(target_os = "macos")]
    let profile_path = PROFILE;
    #[cfg(not(target_os = "macos"))]
    let profile_path = "configuration profile";
    match (file, profile) {
        (Err(e), _) => unreadable(file_path(), e),
        (_, Err(e)) => unreadable(profile_path, e),
        (Ok(Some(_)), Ok(Some(_))) => unreadable(profile_path, format!("a policy file at {} also exists; deploy the policy one way", file_path())),
        (Ok(Some(text)), Ok(None)) => decode(&text).map_or_else(|e| unreadable(file_path(), e), Policy::Document),
        (Ok(None), Ok(Some(text))) => serde_json::from_str::<Value>(&text)
            .map_err(|e| e.to_string())
            .and_then(|v| decode_value(profile_document(v)))
            .map_or_else(|e| unreadable(profile_path, e), Policy::Document),
        (Ok(None), Ok(None)) => Policy::None,
    }
}

/// A profile holds the keys itself or under `Forced[0].mcx_preference_settings`, beside
/// `Payload*` and `_*` bookkeeping keys.
fn profile_document(v: Value) -> Value {
    let settings = v.pointer("/Forced/0/mcx_preference_settings").cloned().unwrap_or(v);
    let Value::Object(map) = settings else { return settings };
    Value::Object(map.into_iter().filter(|(k, _)| !k.starts_with("Payload") && !k.starts_with('_')).collect())
}

fn decode(text: &str) -> Result<Value, String> {
    decode_value(serde_json::from_str(text).map_err(|e| e.to_string())?)
}

/// `ManagedPolicyDocument` with `onExcessProperty: "error"`, for the fields the desktop reads.
fn decode_value(v: Value) -> Result<Value, String> {
    let object = v.as_object().ok_or("the policy is not an object")?;
    let unknown = |map: &Map<String, Value>, known: &[&str]| map.keys().find(|k| !known.contains(&k.as_str())).cloned();
    if let Some(key) = unknown(object, &DOCUMENT_KEYS) {
        return Err(format!("unknown key \"{key}\""));
    }
    if object.get("schemaVersion") != Some(&Value::from(1)) {
        return Err("schemaVersion must be 1".into());
    }
    if !object.get("organization").is_some_and(Value::is_string) {
        return Err("organization must be a string".into());
    }
    match object.get("limits") {
        None => {}
        Some(Value::Object(limits)) => {
            if let Some(key) = unknown(limits, &LIMIT_KEYS) {
                return Err(format!("unknown key \"limits.{key}\""));
            }
            if let Some(clients) = limits.get("clients") {
                let valid = clients.as_array().is_some_and(|c| !c.is_empty() && c.iter().all(|c| matches!(c.as_str(), Some("desktop" | "local-browser" | "remote"))));
                if !valid {
                    return Err("limits.clients must list desktop, local-browser or remote".into());
                }
            }
            if limits.get("browserCookieImport").is_some_and(|b| !b.is_boolean()) {
                return Err("limits.browserCookieImport must be a boolean".into());
            }
        }
        Some(_) => return Err("limits must be an object".into()),
    }
    Ok(v)
}

impl Policy {
    fn organization(&self) -> Option<&str> {
        match self {
            Policy::Document(d) => d["organization"].as_str(),
            _ => None,
        }
    }

    /// `limits.clients` without `remote` keeps the server on loopback and Tailscale Serve off,
    /// or the server refuses to start (`DesktopServerExposure.withinPolicy`).
    pub fn allows_remote(&self) -> bool {
        match self {
            Policy::None => true,
            Policy::Unreadable(..) => false,
            Policy::Document(d) => d["limits"]["clients"].as_array().is_none_or(|c| c.iter().any(|c| c == "remote")),
        }
    }

    /// `BrowserImportPolicyError`'s message when cookie import is refused.
    pub fn denies_cookie_import(&self) -> Option<String> {
        match self {
            Policy::None => None,
            Policy::Unreadable(path, _) => Some(format!("The managed policy at {path} could not be read, so importing browser cookies is not allowed.")),
            Policy::Document(d) if d["limits"]["browserCookieImport"] == false => {
                Some(format!("{}'s policy does not allow importing browser cookies.", self.organization().unwrap_or_default()))
            }
            Policy::Document(_) => None,
        }
    }

    /// Why the network-access setting is held, for its row.
    pub fn exposure_note(&self) -> Option<String> {
        match self {
            _ if self.allows_remote() => None,
            Policy::Unreadable(path, _) => Some(format!("The managed policy at {path} could not be read, so this machine stays local.")),
            _ => Some(format!("{}'s policy does not allow remote access.", self.organization().unwrap_or_default())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(text: &str) -> Policy {
        read(Ok(Some(text.to_owned())), Ok(None))
    }

    #[test]
    fn reads_one_source_and_fails_closed() {
        assert_eq!(read(Ok(None), Ok(None)), Policy::None);
        assert!(Policy::None.allows_remote());
        assert_eq!(Policy::None.denies_cookie_import(), None);

        let local = file(r#"{"schemaVersion":1,"organization":"Acme","limits":{"clients":["desktop"],"browserCookieImport":false}}"#);
        assert!(!local.allows_remote());
        assert_eq!(local.denies_cookie_import().as_deref(), Some("Acme's policy does not allow importing browser cookies."));
        assert_eq!(local.exposure_note().as_deref(), Some("Acme's policy does not allow remote access."));

        let remote = file(r#"{"schemaVersion":1,"organization":"Acme","settings":{},"limits":{"clients":["desktop","remote"]}}"#);
        assert!(remote.allows_remote());
        assert_eq!(remote.denies_cookie_import(), None);
        assert!(file(r#"{"schemaVersion":1,"organization":"Acme"}"#).allows_remote());

        for bad in [
            r#"{"schemaVersion":1,"organization":"Acme","extra":1}"#,
            r#"{"schemaVersion":1,"organization":"Acme","limits":{"cookies":false}}"#,
            r#"{"schemaVersion":2,"organization":"Acme"}"#,
            r#"{"schemaVersion":1}"#,
            r#"{"schemaVersion":1,"organization":"Acme","limits":{"clients":[]}}"#,
            "not json",
        ] {
            let policy = file(bad);
            assert!(matches!(policy, Policy::Unreadable(..)), "{bad}");
            assert!(!policy.allows_remote(), "{bad}");
            assert!(policy.denies_cookie_import().unwrap().contains("could not be read"), "{bad}");
        }
        let both = read(Ok(Some("{}".into())), Ok(Some("{}".into())));
        assert!(matches!(both, Policy::Unreadable(_, ref why) if why.contains("deploy the policy one way")));
        assert!(matches!(read(Err("permission denied".into()), Ok(None)), Policy::Unreadable(..)));
    }

    #[test]
    fn a_profile_holds_the_keys_itself_or_forced() {
        let forced = r#"{"Forced":[{"mcx_preference_settings":{"schemaVersion":1,"organization":"Acme","PayloadUUID":"x","_meta":1,"limits":{"clients":["desktop"]}}}]}"#;
        let policy = read(Ok(None), Ok(Some(forced.into())));
        assert!(matches!(policy, Policy::Document(_)), "{policy:?}");
        assert!(!policy.allows_remote());
        let direct = r#"{"schemaVersion":1,"organization":"Acme","PayloadDisplayName":"T3"}"#;
        assert!(read(Ok(None), Ok(Some(direct.into()))).allows_remote());
    }
}
