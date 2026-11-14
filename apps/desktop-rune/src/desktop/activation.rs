//! Electron's DesktopAppActivation: `t3 app [dir]` asks the running app, over a socket named after
//! the state directory (shared `resolveDesktopAppControlAddress`), to open that folder as a
//! project in a new thread. One JSON line in, one JSON line out (contracts' desktopAppActivation).

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use crate::{App, Route, data, env};

const PROTOCOL_VERSION: u64 = 1;
const MAX_REQUEST_BYTES: u64 = 64 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq)]
struct Request {
    id: String,
    root: String,
    platform: String,
}

/// A project this app asked the server to create, waiting for its reply.
pub struct Pending {
    reply_id: u64,
    project_id: String,
    request_id: String,
    reply: Sender<Value>,
}

#[derive(Default)]
pub struct State {
    pending: Vec<Pending>,
}

unsafe extern "C" {
    fn getuid() -> u32;
}

/// `resolveDesktopAppControlAddress`: a short hash of the state directory keeps the socket path
/// within the platform's limit.
fn address(state_dir: &Path, temp_dir: &Path, uid: u32, macos: bool) -> (PathBuf, PathBuf) {
    let digest = ring::digest::digest(&ring::digest::SHA256, state_dir.to_string_lossy().as_bytes());
    let hash: String = digest.as_ref()[..12].iter().map(|b| format!("{b:02x}")).collect();
    let socket = format!("{hash}.sock");
    let max = if macos { 103 } else { 107 };
    let mut dir = temp_dir.join(format!("t3code-{uid}"));
    if dir.join(&socket).as_os_str().len() > max {
        dir = PathBuf::from("/tmp").join(format!("t3code-{uid}"));
    }
    (dir.join(&socket), dir)
}

/// Node's `os.tmpdir()`: `$TMPDIR` without its trailing slash, else /tmp.
fn temp_dir() -> PathBuf {
    let dir = std::env::var("TMPDIR").ok().map(|d| d.trim_end_matches('/').to_owned()).filter(|d| !d.is_empty());
    PathBuf::from(dir.unwrap_or_else(|| "/tmp".into()))
}

fn bind(path: &Path, dir: &Path, uid: u32) -> std::io::Result<UnixListener> {
    std::fs::create_dir_all(dir)?;
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.is_dir() || meta.uid() != uid {
        return Err(std::io::Error::other(format!("{} is not this user's directory", dir.display())));
    }
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    // Bind beside it and move into place, so a live app's socket is taken over in one step.
    let staging = dir.join(format!("{}.tmp", &s5_net::uuid()[..12]));
    let listener = UnixListener::bind(&staging)?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o600))?;
    std::fs::rename(&staging, path)?;
    Ok(listener)
}

/// Listens for `t3 app` while this app runs its own server; Electron skips it too while the local
/// environment is off.
pub fn serve(app: rune::AppHandle<App>) {
    if rune::is_headless() || !crate::host::local_enabled() {
        return;
    }
    let uid = unsafe { getuid() };
    let state_dir = crate::host::t3_home().join("userdata");
    let state_dir = std::fs::canonicalize(&state_dir).unwrap_or(state_dir);
    let (path, dir) = address(&state_dir, &temp_dir(), uid, cfg!(target_os = "macos"));
    let listener = match bind(&path, &dir, uid) {
        Ok(listener) => listener,
        Err(error) => return eprintln!("s5code-rune: could not start the app control socket at {}: {error}", path.display()),
    };
    let app = Arc::new(Mutex::new(app));
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let app = app.clone();
            std::thread::spawn(move || answer(&app, stream));
        }
    });
}

fn answer(app: &Mutex<rune::AppHandle<App>>, mut stream: UnixStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    let read = BufReader::new((&stream).take(MAX_REQUEST_BYTES)).read_line(&mut line);
    let response = match read.map_err(|_| "The desktop app request could not be read.").and_then(|_| parse(&line)) {
        Err(message) => failure(&request_id(&line), "invalid-request", message),
        Ok(request) => {
            let (tx, rx) = channel();
            let id = request.id.clone();
            if let Ok(app) = app.lock() {
                app.activate(String::new());
                app.update(move |s, _| handle(s, request, tx));
            }
            rx.recv_timeout(REQUEST_TIMEOUT).unwrap_or_else(|_| failure(&id, "request-timeout", "T3 Code did not answer the desktop app request in time."))
        }
    };
    let _ = writeln!(stream, "{response}");
}

fn request_id(line: &str) -> String {
    serde_json::from_str::<Value>(line).ok().and_then(|v| v["requestId"].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)).unwrap_or_else(|| "invalid-request".into())
}

/// `isDesktopAppActivationRequest`.
fn parse(line: &str) -> Result<Request, &'static str> {
    if !line.ends_with('\n') {
        return Err("The desktop app request is too large.");
    }
    let v: Value = serde_json::from_str(line).map_err(|_| "The desktop app request is not valid JSON.")?;
    let text = |key: &str| v[key].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned);
    match (v["version"].as_u64(), v["type"].as_str(), text("requestId"), text("workspaceRoot"), v["platform"].as_str()) {
        (Some(PROTOCOL_VERSION), Some("open-workspace"), Some(id), Some(root), Some(platform @ ("darwin" | "linux" | "win32"))) => {
            Ok(Request { id, root, platform: platform.to_owned() })
        }
        _ => Err("The desktop app request is invalid."),
    }
}

fn failure(request_id: &str, code: &str, message: &str) -> Value {
    json!({ "version": PROTOCOL_VERSION, "requestId": request_id, "ok": false, "code": code, "message": message })
}

fn success(request_id: &str, project_id: &str) -> Value {
    json!({ "version": PROTOCOL_VERSION, "requestId": request_id, "ok": true, "projectId": project_id, "threadId": s5_net::uuid() })
}

fn same_root(a: &str, b: &str) -> bool {
    let trim = |p: &str| if p.len() > 1 { p.trim_end_matches('/').to_owned() } else { p.to_owned() };
    trim(a) == trim(b)
}

/// Web's `handleDesktopAppActivationRequest`, against this machine's environment.
fn handle(s: &mut App, request: Request, reply: Sender<Value>) {
    let send = |value: Value| {
        let _ = reply.send(value);
    };
    let config = env::source(s, env::LOCAL).filter(|_| !s.ui.desktop.local_off && env::connected(s, env::LOCAL)).and_then(|src| src.state().config.clone());
    let Some(config) = config else {
        return send(failure(&request.id, "environment-unavailable", "The desktop app's primary local environment is not connected."));
    };
    let wanted = if request.platform == "win32" { "windows" } else { request.platform.as_str() };
    let os = config["environment"]["platform"]["os"].as_str().unwrap_or("");
    if os != wanted {
        return send(failure(
            &request.id,
            "platform-mismatch",
            &format!("The command path is for {wanted}, but the desktop app's primary environment uses {os}. Cross-platform path mapping is not supported."),
        ));
    }
    let existing = env::source(s, env::LOCAL)
        .and_then(|src| src.state().projects.iter().find(|p| p["archivedAt"].is_null() && same_root(data::s(p, "workspaceRoot"), &request.root)).map(|p| data::s(p, "id").to_owned()));
    if let Some(project_id) = existing {
        crate::shell::sidebar::go(s, Route::Home { project_id: Some(project_id.clone()) });
        return send(success(&request.id, &project_id));
    }
    let project_id = s5_net::uuid();
    let title = Path::new(&request.root).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| request.root.clone());
    let command = json!({
        "type": "project.create",
        "commandId": s5_net::uuid(),
        "projectId": project_id,
        "title": title,
        "workspaceRoot": request.root,
        "createWorkspaceRootIfMissing": false,
        "defaultModelSelection": null,
    });
    let Some(net) = env::net_of(s, env::LOCAL) else {
        return send(failure(&request.id, "environment-unavailable", "The desktop app's primary local environment is not connected."));
    };
    let reply_id = net.request("projects.mutate", command);
    s.ui.desktop.activation.pending.push(Pending { reply_id, project_id, request_id: request.id, reply });
}

/// Lands `project.create` replies: open the new project, or say why it failed.
pub fn poll(s: &mut App) -> bool {
    if s.ui.desktop.activation.pending.is_empty() {
        return false;
    }
    let mut landed = Vec::new();
    let pending = std::mem::take(&mut s.ui.desktop.activation.pending);
    for p in pending {
        match env::net_of(s, env::LOCAL).and_then(|net| net.take_reply(p.reply_id)) {
            Some(reply) => landed.push((p, reply)),
            None => s.ui.desktop.activation.pending.push(p),
        }
    }
    let changed = !landed.is_empty();
    for (p, reply) in landed {
        let response = match reply {
            Ok(_) => {
                crate::shell::sidebar::go(s, Route::Home { project_id: Some(p.project_id.clone()) });
                success(&p.request_id, &p.project_id)
            }
            Err(e) => failure(&p.request_id, "project-create-failed", &crate::shell::palette::error_message(&e, "T3 Code could not add the project.")),
        };
        let _ = p.reply.send(response);
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_address_matches_the_t3_cli() {
        // `resolveDesktopAppControlAddress` for this state directory, from packages/shared.
        let (path, dir) = address(Path::new("/home/me/.t3/userdata"), Path::new("/tmp"), 1000, false);
        assert_eq!(dir, Path::new("/tmp/t3code-1000"));
        assert_eq!(path, Path::new("/tmp/t3code-1000/7f1d40cb2ca3536e019e461d.sock"));
        // macOS's long TMPDIR falls back to /tmp past 103 bytes.
        let long = Path::new("/var/folders/zz/abcdefghijklmnopqrstuvwxyz0123456789abcdefghijklmnopqrstuvwxyz/T");
        assert_eq!(address(Path::new("/s"), long, 501, true).1, Path::new("/tmp/t3code-501"));
    }

    #[test]
    fn only_a_well_formed_open_workspace_request_is_accepted() {
        let ok = r#"{"version":1,"requestId":"r1","type":"open-workspace","workspaceRoot":" /src/app ","platform":"linux"}"#;
        assert_eq!(parse(&format!("{ok}\n")), Ok(Request { id: "r1".into(), root: "/src/app".into(), platform: "linux".into() }));
        assert_eq!(parse(ok), Err("The desktop app request is too large."), "no newline within the limit");
        assert_eq!(parse("{\n"), Err("The desktop app request is not valid JSON."));
        for bad in [
            r#"{"version":2,"requestId":"r1","type":"open-workspace","workspaceRoot":"/a","platform":"linux"}"#,
            r#"{"version":1,"requestId":" ","type":"open-workspace","workspaceRoot":"/a","platform":"linux"}"#,
            r#"{"version":1,"requestId":"r1","type":"open-file","workspaceRoot":"/a","platform":"linux"}"#,
            r#"{"version":1,"requestId":"r1","type":"open-workspace","workspaceRoot":"/a","platform":"freebsd"}"#,
        ] {
            assert_eq!(parse(&format!("{bad}\n")), Err("The desktop app request is invalid."), "{bad}");
        }
        assert_eq!(request_id(r#"{"requestId":"r9","version":3}"#), "r9");
        assert_eq!(request_id("nope"), "invalid-request");
        assert!(same_root("/src/app/", "/src/app") && !same_root("/src/app", "/src/apps"));
    }
}
