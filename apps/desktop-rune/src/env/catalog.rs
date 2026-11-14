//! Electron's saved connections (`connection-catalog.json`, DesktopConnectionCatalogStore), so
//! a backend paired in either app shows in both. The whole catalog is one safeStorage blob.
//! Rune reads the key Electron stored (the macOS Keychain through `security`, libsecret through
//! `secret-tool`) and joins a catalog that already exists. It never makes a key, since a blob
//! Electron can't decrypt would cost it every saved connection.
//!
//! Bearer connections sync both ways. SSH, relay and learned entries are kept as they are.

use std::path::PathBuf;
use std::sync::Mutex;

use base64::Engine;
use serde_json::{Value, json};

use super::Saved;
use crate::panel::browser_import::{decrypt_cbc, derive, encrypt_cbc};

/// The names safeStorage may have used: `app.setName` runs before ready, and a packaged build
/// may still have keyed it with its product name.
const NAMES: [&str; 6] = ["S5 Code Beta", "S5 Code (Beta)", "S5 Code Nightly", "S5 Code (Nightly)", "S5 Code Dev", "S5 Code"];

type Key = ([u8; 3], [u8; 16]);
/// The key that opened the catalog this launch, so saving never asks the keychain again.
static KEY: Mutex<Option<Key>> = Mutex::new(None);

fn path() -> PathBuf {
    crate::host::t3_home().join("userdata").join("connection-catalog.json")
}

#[cfg(target_os = "macos")]
fn candidates() -> Vec<Key> {
    NAMES.iter().filter_map(|name| crate::panel::browser_import::keychain_service(&format!("{name} Safe Storage")).ok()).map(|pw| (*b"v10", derive(pw.as_bytes(), 1003))).collect()
}

#[cfg(not(target_os = "macos"))]
fn candidates() -> Vec<Key> {
    NAMES.iter().filter_map(|name| crate::panel::browser_import::linux_secret(name).ok()).map(|secret| (*b"v11", derive(secret.as_bytes(), 1))).collect()
}

/// The inner catalog, with the key that opened it.
fn open(text: &str, keys: impl IntoIterator<Item = Key>) -> Option<(Value, Key)> {
    let outer = crate::settings::client::lenient_json(text)?;
    let sealed = base64::engine::general_purpose::STANDARD.decode(outer["encryptedCatalog"].as_str()?).ok()?;
    let (prefix, payload) = (sealed.get(..3)?, sealed.get(3..)?);
    keys.into_iter().filter(|(p, _)| p == prefix).find_map(|key| {
        let inner: Value = serde_json::from_str(&decrypt_cbc(payload, &key.1, "", 0)?).ok()?;
        (inner["schemaVersion"] == 1).then_some((inner, key))
    })
}

fn seal(inner: &Value, (prefix, key): Key) -> String {
    let mut sealed = prefix.to_vec();
    sealed.extend(encrypt_cbc(inner.to_string().as_bytes(), &key));
    format!("{}\n", json!({"version": 1, "encryptedCatalog": base64::engine::general_purpose::STANDARD.encode(sealed)}))
}

fn list<'a>(inner: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    inner[key].as_array().into_iter().flatten()
}

/// Bearer connections a client can reconnect with: a target, its profile and its token.
fn bearers(inner: &Value) -> Vec<Saved> {
    list(inner, "targets")
        .filter(|t| t["_tag"] == "BearerConnectionTarget")
        .filter_map(|t| {
            let id = t["connectionId"].as_str()?;
            let profile = list(inner, "profiles").find(|p| p["connectionId"] == id && p["_tag"] == "BearerConnectionProfile" && p["environmentId"] == t["environmentId"])?;
            if profile["authorization"] == "t3-connect" {
                return None;
            }
            let token = list(inner, "credentials").find(|c| c["connectionId"] == id)?["credential"]["token"].as_str()?;
            Some(Saved {
                id: t["environmentId"].as_str()?.to_owned(),
                label: t["label"].as_str().unwrap_or_default().to_owned(),
                base: profile["httpBaseUrl"].as_str()?.trim_end_matches('/').to_owned(),
                bearer: token.to_owned(),
            })
        })
        .filter(|e| !e.id.is_empty() && !e.base.is_empty() && !e.bearer.is_empty())
        .collect()
}

/// `normalizeAdvertisedEndpoint`: the origin with a `/` path, and its WebSocket twin.
fn endpoints(base: &str) -> Option<(String, String)> {
    let url = s5_net::url::Url::parse(base).ok()?;
    let http = format!("{}/", url.origin().ascii_serialization());
    let ws = http.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    Some((http, ws))
}

/// SSH tunnels are loopback ports that only live with this process, so they stay out.
fn exportable(e: &Saved) -> bool {
    s5_net::url::Url::parse(&e.base).is_ok_and(|u| !matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]")))
}

/// Mirrors Rune's bearer list into the catalog: each saved one upserted, each one removed
/// since the last save dropped.
fn merge(inner: &mut Value, old: &[Saved], new: &[Saved]) {
    let removed: Vec<&str> = old.iter().filter(|o| !new.iter().any(|n| n.id == o.id)).map(|o| o.id.as_str()).collect();
    let gone: Vec<String> = list(inner, "targets")
        .filter(|t| t["_tag"] == "BearerConnectionTarget" && t["environmentId"].as_str().is_some_and(|id| removed.contains(&id)))
        .filter_map(|t| t["connectionId"].as_str().map(str::to_owned))
        .collect();
    for key in ["targets", "profiles", "credentials"] {
        if let Some(rows) = inner[key].as_array_mut() {
            rows.retain(|r| !r["connectionId"].as_str().is_some_and(|id| gone.iter().any(|g| g == id)));
        }
    }
    for e in new.iter().filter(|e| exportable(e)) {
        let Some((http, ws)) = endpoints(&e.base) else { continue };
        let existing = list(inner, "targets").find(|t| t["_tag"] == "BearerConnectionTarget" && t["environmentId"] == e.id.as_str()).and_then(|t| t["connectionId"].as_str()).map(str::to_owned);
        let id = existing.unwrap_or_else(|| format!("bearer:{}:{}", e.id, http.trim_end_matches('/')));
        let rows = |inner: &mut Value, key: &str| -> &mut Vec<Value> {
            if !inner[key].is_array() {
                inner[key] = json!([]);
            }
            inner[key].as_array_mut().expect("made an array above")
        };
        let targets = rows(inner, "targets");
        match targets.iter_mut().find(|t| t["connectionId"] == id.as_str()) {
            Some(t) => t["label"] = json!(e.label),
            None => targets.push(json!({"_tag": "BearerConnectionTarget", "environmentId": e.id, "label": e.label, "connectionId": id})),
        }
        let profiles = rows(inner, "profiles");
        match profiles.iter_mut().find(|p| p["connectionId"] == id.as_str()) {
            Some(p) => {
                p["label"] = json!(e.label);
                p["httpBaseUrl"] = json!(http);
                p["wsBaseUrl"] = json!(ws);
            }
            None => profiles.push(json!({"_tag": "BearerConnectionProfile", "connectionId": id, "environmentId": e.id, "label": e.label, "httpBaseUrl": http, "wsBaseUrl": ws})),
        }
        let credential = json!({"_tag": "BearerConnectionCredential", "token": e.bearer});
        let credentials = rows(inner, "credentials");
        match credentials.iter_mut().find(|c| c["connectionId"] == id.as_str()) {
            Some(c) => c["credential"] = credential,
            None => credentials.push(json!({"connectionId": id, "credential": credential})),
        }
    }
}

/// The catalog's bearer connections, remembering the key for saves. Blocking: the keychain may
/// ask the person first.
pub fn read() -> Vec<Saved> {
    let Ok(text) = std::fs::read_to_string(path()) else { return Vec::new() };
    let Some((inner, key)) = open(&text, candidates()) else { return Vec::new() };
    *KEY.lock().unwrap() = Some(key);
    bearers(&inner)
}

/// After Rune saves its list: the catalog follows, read fresh since Electron may have written.
pub fn export(old: &[Saved], new: &[Saved]) {
    let Some(key) = *KEY.lock().unwrap() else { return };
    let path = path();
    let Some((mut inner, _)) = std::fs::read_to_string(&path).ok().and_then(|text| open(&text, [key])) else { return };
    merge(&mut inner, old, new);
    let temporary = path.with_file_name(format!("connection-catalog.json.{}.{}.tmp", std::process::id(), s5_net::uuid().replace('-', "")));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&temporary).and_then(|mut f| std::io::Write::write_all(&mut f, seal(&inner, key).as_bytes())).and_then(|_| std::fs::rename(&temporary, &path));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(id: &str, base: &str, bearer: &str) -> Saved {
        Saved { id: id.into(), label: format!("{id} box"), base: base.into(), bearer: bearer.into() }
    }

    fn catalog() -> Value {
        json!({"schemaVersion": 1,
            "targets": [
                {"_tag": "BearerConnectionTarget", "environmentId": "env-abc", "label": "Build box", "connectionId": "bearer:env-abc:https://box.example.com"},
                {"_tag": "SshConnectionTarget", "environmentId": "env-ssh", "label": "devbox", "connectionId": "ssh:env-ssh"},
                {"_tag": "RelayConnectionTarget", "environmentId": "env-relay", "label": "Relay"}],
            "profiles": [
                {"_tag": "BearerConnectionProfile", "connectionId": "bearer:env-abc:https://box.example.com", "environmentId": "env-abc", "label": "Build box", "httpBaseUrl": "https://box.example.com/", "wsBaseUrl": "wss://box.example.com/"},
                {"_tag": "SshConnectionProfile", "connectionId": "ssh:env-ssh", "environmentId": "env-ssh", "label": "devbox", "target": {"alias": "devbox", "hostname": "devbox", "username": null, "port": null}}],
            "credentials": [
                {"connectionId": "bearer:env-abc:https://box.example.com", "credential": {"_tag": "BearerConnectionCredential", "token": "tok-abc"}},
                {"connectionId": "ssh:env-ssh", "credential": {"_tag": "BearerConnectionCredential", "token": "tok-ssh"}}],
            "remoteDpopTokens": [], "disabledEnvironmentIds": ["env-relay"]})
    }

    #[test]
    fn opens_only_with_the_key_that_sealed_it() {
        let key = (*b"v11", derive(b"libsecret secret", 1));
        let text = seal(&catalog(), key);
        assert!(text.ends_with('\n'));
        let (inner, used) = open(&text, [(*b"v11", derive(b"wrong", 1)), key]).expect("the right key opens it");
        assert_eq!((inner, used), (catalog(), key));
        assert!(open(&text, [(*b"v10", key.1)]).is_none(), "a key for the other prefix is never tried");
        assert!(open(&text, [(*b"v11", derive(b"wrong", 1))]).is_none());
        let lenient = text.replacen('{', "{ // Electron reads comments\n", 1).replace("\"version\":1}", "\"version\":1,}");
        assert_ne!(lenient, text);
        assert!(open(&lenient, [key]).is_some());
    }

    #[test]
    fn reads_bearer_connections_and_writes_them_back_in_place() {
        let mut inner = catalog();
        assert_eq!(bearers(&inner), [Saved { id: "env-abc".into(), label: "Build box".into(), base: "https://box.example.com".into(), bearer: "tok-abc".into() }]);

        let renamed = Saved { label: "Renamed".into(), bearer: "tok-new".into(), ..bearers(&inner)[0].clone() };
        let added = saved("env-new", "http://10.0.0.5:3773", "tok-x");
        let tunnel = saved("env-ssh2", "http://127.0.0.1:40001", "tok-t");
        merge(&mut inner, &[], &[renamed, added.clone(), tunnel]);
        let read = bearers(&inner);
        assert_eq!(read.len(), 2, "a loopback SSH tunnel stays out: {read:?}");
        assert_eq!((read[0].label.as_str(), read[0].bearer.as_str()), ("Renamed", "tok-new"));
        assert_eq!(read[1], Saved { label: "env-new box".into(), ..added.clone() });
        let profile = list(&inner, "profiles").find(|p| p["environmentId"] == "env-new").unwrap();
        assert_eq!((profile["connectionId"].as_str(), profile["httpBaseUrl"].as_str(), profile["wsBaseUrl"].as_str()), (Some("bearer:env-new:http://10.0.0.5:3773"), Some("http://10.0.0.5:3773/"), Some("ws://10.0.0.5:3773/")));

        merge(&mut inner, &[added.clone()], &[]);
        assert!(bearers(&inner).iter().all(|e| e.id != "env-new"), "removed in Rune, removed here");
        assert!(list(&inner, "credentials").all(|c| c["connectionId"] != "bearer:env-new:http://10.0.0.5:3773"));
        assert_eq!(list(&inner, "targets").filter(|t| t["_tag"] != "BearerConnectionTarget").count(), 2, "SSH and relay entries stay");
        assert_eq!(inner["disabledEnvironmentIds"], json!(["env-relay"]));
    }
}
