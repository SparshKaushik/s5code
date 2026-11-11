//! Electron's DesktopLinuxUrlHandler: `s5code://` links open this binary wherever it runs, through
//! a hidden entry in the user's applications folder and `xdg-mime default`. Its own file name, so
//! it never hides an installed launcher entry.

use std::path::PathBuf;
use std::process::{Command, Stdio};

const SCHEME: &str = "s5code";

pub fn register() {
    if rune::is_headless() {
        return;
    }
    let (Ok(exe), Some(dir)) = (std::env::current_exe(), applications_dir()) else { return };
    std::thread::spawn(move || {
        let name = format!("{}-url-handler.desktop", super::APP_ID);
        let path = dir.join(&name);
        let content = entry(&exe.to_string_lossy());
        if std::fs::read_to_string(&path).is_ok_and(|c| c == content) && is_default(&name) {
            return;
        }
        if std::fs::create_dir_all(&dir).is_err() || std::fs::write(&path, &content).is_err() {
            return;
        }
        let quiet = |c: &mut Command| c.stdout(Stdio::null()).stderr(Stdio::null()).status();
        let _ = quiet(Command::new("update-desktop-database").arg(&dir));
        let _ = quiet(Command::new("xdg-mime").args(["default", &name, &format!("x-scheme-handler/{SCHEME}")]));
    });
}

fn is_default(name: &str) -> bool {
    Command::new("xdg-mime")
        .args(["query", "default", &format!("x-scheme-handler/{SCHEME}")])
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == name)
}

fn applications_dir() -> Option<PathBuf> {
    let data = match std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => PathBuf::from(std::env::var_os("HOME")?).join(".local/share"),
    };
    Some(data.join("applications"))
}

fn entry(exec: &str) -> String {
    [
        "[Desktop Entry]",
        "Type=Application",
        "Name=S5 Code",
        &format!("Exec={} %U", exec_target(exec)),
        "Terminal=false",
        "NoDisplay=true",
        "StartupNotify=false",
        &format!("MimeType=x-scheme-handler/{SCHEME};"),
        "",
    ]
    .join("\n")
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t")
}

/// The Desktop Entry spec's quoting for an `Exec` program, as Electron's `escapeUrlHandlerExecTarget`.
fn exec_target(path: &str) -> String {
    if path.chars().any(|c| " \t\n\"'\\><~|&;$*?#()`".contains(c)) {
        let quoted = path.replace('\\', "\\\\").replace('`', "\\`").replace('$', "\\$").replace('"', "\\\"").replace('%', "%%");
        escape(&format!("\"{quoted}\""))
    } else {
        escape(&path.replace('%', "%%"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handler_entry_quotes_its_program_as_the_desktop_entry_spec_wants() {
        assert_eq!(exec_target("/opt/s5/s5code-rune"), "/opt/s5/s5code-rune");
        assert_eq!(exec_target("/opt/s5%/rune"), "/opt/s5%%/rune");
        assert_eq!(exec_target("/home/a b/$x/rune"), r#""/home/a b/\\$x/rune""#);
        let entry = entry("/home/a b/rune");
        assert!(entry.contains("Exec=\"/home/a b/rune\" %U\n"), "{entry}");
        assert!(entry.contains("MimeType=x-scheme-handler/s5code;\n") && entry.contains("NoDisplay=true\n"));
    }
}
