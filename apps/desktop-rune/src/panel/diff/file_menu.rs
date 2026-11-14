//! A file header's right-click menu, as web's `fileContextMenu`: Open, Reveal, and Open with.
use rune::{El, Tx};
use serde_json::{Value, json};

use crate::{App, data, kit, theme::T};

#[derive(Default)]
pub struct State {
    /// The file and where the menu opened.
    at: Option<(String, (f32, f32))>,
    sub: bool,
    /// `shell.openInEditor` requests and the toast title and path a failure shows.
    pending: Vec<(u64, String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
enum Action {
    Open,
    Reveal,
    With(String),
}

fn label(id: &str) -> &str {
    match id {
        "cursor" => "Cursor",
        "trae" => "Trae",
        "kiro" => "Kiro",
        "vscode" => "VS Code",
        "vscode-insiders" => "VS Code Insiders",
        "vscodium" => "VSCodium",
        "zed" => "Zed",
        "antigravity" => "Antigravity",
        "idea" => "IntelliJ IDEA",
        "aqua" => "Aqua",
        "clion" => "CLion",
        "datagrip" => "DataGrip",
        "dataspell" => "DataSpell",
        "goland" => "GoLand",
        "phpstorm" => "PhpStorm",
        "pycharm" => "PyCharm",
        "rider" => "Rider",
        "rubymine" => "RubyMine",
        "rustrover" => "RustRover",
        "webstorm" => "WebStorm",
        "file-manager" => "File Manager",
        other => other,
    }
}

/// `buildFileContextMenuItems`: whether Open shows, the reveal label, and the Open with editors.
fn items(config: &Value) -> (bool, Option<&'static str>, Vec<String>) {
    let editors: Vec<String> = config["availableEditors"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| e.as_str().map_or_else(|| data::s(e, "id").to_string(), str::to_string))
        .collect();
    let manager = editors.iter().any(|e| e == "file-manager");
    // WSL reveals in Windows Explorer while reporting Linux, so the server's kind wins.
    let reveal = (manager && config["shellRevealInFileManager"] == true).then(|| match config["shellRevealInFileManagerKind"].as_str() {
        Some("finder") => "Reveal in Finder",
        Some("file-explorer") => "Reveal in File Explorer",
        Some(_) => "Reveal in Files",
        None => match config["environment"]["platform"]["os"].as_str() {
            Some("darwin") => "Reveal in Finder",
            Some("windows") => "Reveal in File Explorer",
            _ => "Reveal in Files",
        },
    });
    (manager, reveal, editors.into_iter().filter(|e| e != "file-manager").collect())
}

fn windows_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':'
}

/// Path segments without `.`; `None` for an absolute path or one that climbs out with `..`.
fn relative_segments(path: &str) -> Option<Vec<String>> {
    if path.starts_with('/') || windows_absolute(path) {
        return None;
    }
    let segments: Vec<String> = path.replace('\\', "/").split('/').filter(|s| !s.is_empty() && *s != ".").map(str::to_string).collect();
    (!segments.is_empty() && !segments.iter().any(|s| s == "..")).then_some(segments)
}

/// `workspaceRelativeFilePath`: an absolute path under `root`, relative to it.
fn under(path: &str, root: &str) -> Option<String> {
    let fold = windows_absolute(root.trim_start_matches('/'));
    let norm = |p: &str| if fold { p.replace('\\', "/").to_ascii_lowercase() } else { p.to_string() };
    let root = norm(root.trim_end_matches(['/', '\\']));
    let full = norm(path);
    full.strip_prefix(&format!("{root}/")).map(|rest| path[path.len() - rest.len()..].replace('\\', "/"))
}

/// `resolveDiffPathForWorkspace`: a diff names files by repository-relative path (or absolute,
/// from Claude and ACP tool calls); the workspace may be a folder inside the repository.
pub fn resolve_diff_path(file: &str, workspace: Option<&str>, repository: Option<&str>) -> Option<String> {
    if file.starts_with('/') || windows_absolute(file) {
        return relative_segments(&under(file, workspace?)?).map(|s| s.join("/"));
    }
    let segments = relative_segments(file)?;
    let inner = match (workspace, repository) {
        (Some(w), Some(r)) if w.trim_end_matches('/') != r.trim_end_matches('/') => under(w, r)?,
        _ => return Some(segments.join("/")),
    };
    let fold = repository.is_some_and(windows_absolute);
    let prefix: Vec<&str> = inner.split('/').filter(|s| !s.is_empty()).collect();
    let belongs = prefix.len() < segments.len()
        && prefix.iter().zip(&segments).all(|(p, s)| if fold { p.eq_ignore_ascii_case(s) } else { p == s });
    belongs.then(|| segments[prefix.len()..].join("/"))
}

pub fn open(app: &mut App, path: String, at: (f32, f32)) {
    let ui = &mut app.ui.panel.diff.file_menu;
    (ui.at, ui.sub) = (Some((path, at)), false);
}

fn close(app: &mut App) {
    app.ui.panel.diff.file_menu.at = None;
}

fn run(app: &mut App, path: &str, action: Action) {
    close(app);
    let Some((_, cwd)) = crate::panel::diff_context(app) else { return };
    let Some(relative) = crate::panel::workspace_path(app, path) else { return };
    let absolute = format!("{}/{relative}", cwd.trim_end_matches('/'));
    let (editor, title) = match &action {
        Action::Open => ("file-manager".to_string(), "Could not open file".to_string()),
        Action::Reveal => ("file-manager".to_string(), "Unable to reveal file".to_string()),
        Action::With(id) => (id.clone(), format!("Could not open in {}", label(id))),
    };
    let mut request = json!({"cwd": absolute, "editor": editor});
    if action == Action::Reveal {
        request["reveal"] = json!(true);
    }
    if let Some(net) = app.src.net() {
        let id = net.request("shell.openInEditor", request);
        app.ui.panel.diff.file_menu.pending.push((id, title, absolute));
    }
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    for (id, title, path) in std::mem::take(&mut app.ui.panel.diff.file_menu.pending) {
        match app.src.net().and_then(|net| net.take_reply(id)) {
            None => app.ui.panel.diff.file_menu.pending.push((id, title, path)),
            Some(reply) => {
                if reply.is_err() {
                    crate::shell::palette::toast_error(app, &title, Some(&path));
                }
                changed = true;
            }
        }
    }
    changed
}

pub fn overlay(app: &App, t: &T, out: &mut Vec<El>) {
    use kit::{MenuEntry, MenuItem};
    let Some((path, (x, y))) = app.ui.panel.diff.file_menu.at.clone() else { return };
    let config = app.state().config.clone().unwrap_or(Value::Null);
    let (open, reveal, editors) = items(&config);
    let act = |action: Action| {
        let path = path.clone();
        move |s: &mut App, _: &mut Tx| run(s, &path, action.clone())
    };
    let mut entries = vec![];
    if open {
        entries.push(MenuEntry::item(MenuItem::new("Open").on_select_tx(act(Action::Open))));
    }
    if let Some(reveal) = reveal {
        entries.push(MenuEntry::item(MenuItem::new(reveal).on_select_tx(act(Action::Reveal))));
    }
    if !editors.is_empty() {
        let with = editors.iter().map(|id| MenuEntry::item(MenuItem::new(label(id)).on_select_tx(act(Action::With(id.clone()))))).collect();
        entries.push(MenuEntry::item(
            MenuItem::new("Open with")
                .submenu(with, app.ui.panel.diff.file_menu.sub)
                .on_submenu_change(|s: &mut App, on| s.ui.panel.diff.file_menu.sub = on),
        ));
    }
    if entries.is_empty() {
        return;
    }
    kit::Menu::new("panel-diff-file-menu").native(true).width(198.).entries(entries).open(true).at(x, y).on_close(close).overlay(&kit::theme(t), out);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_follow_the_servers_editors_and_reveal_support() {
        let config = json!({"availableEditors": ["cursor", "file-manager", "zed"], "shellRevealInFileManager": true, "environment": {"platform": {"os": "darwin"}}});
        assert_eq!(items(&config), (true, Some("Reveal in Finder"), vec!["cursor".to_string(), "zed".to_string()]));
        let wsl = json!({"availableEditors": ["file-manager"], "shellRevealInFileManager": true, "shellRevealInFileManagerKind": "file-explorer", "environment": {"platform": {"os": "linux"}}});
        assert_eq!(items(&wsl), (true, Some("Reveal in File Explorer"), vec![]));
        let bare = json!({"availableEditors": ["vscode"], "shellRevealInFileManager": true});
        assert_eq!(items(&bare), (false, None, vec!["vscode".to_string()]));
        assert_eq!(label("vscode"), "VS Code");
    }

    #[test]
    fn diff_paths_resolve_against_a_workspace_inside_the_repository() {
        assert_eq!(resolve_diff_path("src/a.rs", Some("/repo"), Some("/repo")).as_deref(), Some("src/a.rs"));
        assert_eq!(resolve_diff_path("./src//a.rs", Some("/repo"), None).as_deref(), Some("src/a.rs"));
        assert_eq!(resolve_diff_path("app/src/a.rs", Some("/repo/app"), Some("/repo")).as_deref(), Some("src/a.rs"));
        assert_eq!(resolve_diff_path("other/a.rs", Some("/repo/app"), Some("/repo")), None, "outside the workspace");
        assert_eq!(resolve_diff_path("app", Some("/repo/app"), Some("/repo")), None, "the workspace itself");
        assert_eq!(resolve_diff_path("../etc/passwd", Some("/repo"), None), None);
        assert_eq!(resolve_diff_path("/repo/app/src/a.rs", Some("/repo/app"), None).as_deref(), Some("src/a.rs"));
        assert_eq!(resolve_diff_path("/elsewhere/a.rs", Some("/repo"), None), None);
        assert_eq!(resolve_diff_path("/repo/a.rs", None, None), None);
        assert_eq!(resolve_diff_path("C:\\Repo\\App\\x.rs", Some("c:/repo"), None).as_deref(), Some("App/x.rs"));
    }
}
