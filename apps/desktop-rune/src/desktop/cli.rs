//! Settings › About › t3 command, as Electron's DesktopCliCommand on macOS and Linux: a launcher
//! in the T3 home's `bin` runs the bundled `s5code-server` (the t3 CLI), and a `t3` link to it
//! goes in the first writable candidate folder, those on PATH first.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::App;

/// Electron's marker, so either app recognises and replaces the other's launcher.
const MARKER: &str = "Written by S5 Code: runs the desktop app's bundled t3 CLI.";
const MOVED: &str = "S5 Code has moved or been removed. Open the app once to update this command.";

#[derive(Default)]
pub struct State {
    /// None without a bundled server, which is when Electron reports the command unsupported.
    status: Option<Status>,
    error: Option<String>,
    /// Whether `status` saw the login shell's PATH, which a terminal will have.
    pub(super) login_path: bool,
}

#[derive(Clone, PartialEq, Debug)]
struct Status {
    installed: Option<PathBuf>,
    on_path: bool,
    shadowed_by: Option<PathBuf>,
}

impl State {
    /// Keeps an installed launcher pointing at this build's server, as Electron does each launch.
    pub(super) fn load(&mut self) {
        self.login_path = super::shell_path::get().is_some();
        let Some((paths, script)) = current() else { return };
        if paths.launcher.exists() {
            let _ = write_launcher(&paths.launcher, &script);
        }
        self.status = Some(paths.status());
    }
}

/// What the About page shows: the description, the button and whether it is disabled.
pub struct Row {
    pub description: String,
    pub action: &'static str,
    pub disabled: bool,
}

pub fn row(s: &App) -> Option<Row> {
    let ui = &s.ui.desktop.cli;
    let status = ui.status.as_ref()?;
    let installed = status.installed.as_ref().map(|p| p.display().to_string());
    let description = match (&ui.error, &status.shadowed_by, &installed) {
        (Some(error), ..) => error.clone(),
        (None, Some(other), _) => format!("Another t3 at {} runs first in a new terminal. Remove it to use T3 Code's.", other.display()),
        (None, None, None) => "Run T3 Code's CLI as `t3` from any terminal.".into(),
        (None, None, Some(at)) if status.on_path => format!("Installed at {at}. Open a new terminal to use it."),
        (None, None, Some(at)) => format!("Installed at {at}, which is not on your PATH yet. Add its folder to your PATH to run `t3`."),
    };
    Some(Row {
        description,
        action: if installed.is_some() { "Remove" } else { "Install" },
        disabled: installed.is_none() && status.shadowed_by.is_some(),
    })
}

pub fn press(s: &mut App) {
    let Some((paths, script)) = current() else { return };
    let installed = s.ui.desktop.cli.status.as_ref().is_some_and(|st| st.installed.is_some());
    let result = if installed { paths.uninstall() } else { paths.install(&script) };
    let ui = &mut s.ui.desktop.cli;
    ui.error = result.err();
    ui.status = Some(paths.status());
}

fn current() -> Option<(Paths, String)> {
    let binary = crate::host::bundled_binary()?;
    let home = PathBuf::from(std::env::var_os("HOME")?);
    let t3_home = crate::host::t3_home();
    let paths = Paths {
        launcher: t3_home.join("bin").join("t3"),
        candidates: candidates(&home),
        path_env: super::shell_path::current(),
    };
    let script = launcher_script(&paths.launcher, &t3_home, &binary);
    Some((paths, script))
}

fn candidates(home: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = if cfg!(target_os = "macos") { vec!["/opt/homebrew/bin".into(), "/usr/local/bin".into()] } else { vec![] };
    dirs.extend([home.join(".local/bin"), home.join("bin")]);
    dirs
}

fn shell_word(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn launcher_script(launcher: &Path, t3_home: &Path, binary: &Path) -> String {
    let word = |p: &Path| shell_word(&p.to_string_lossy());
    format!(
        "#!/bin/sh\n# {MARKER}\nexport T3CODE_CLI_PATH={}\nhome={}\nexport T3CODE_HOME=\"${{T3CODE_HOME:-$home}}\"\napp={}\nif [ ! -x \"$app\" ]; then\n  echo \"{MOVED}\" >&2\n  exit 127\nfi\nexec \"$app\" \"$@\"\n",
        word(launcher),
        word(t3_home),
        word(binary),
    )
}

/// Replaces only a launcher an app wrote.
fn write_launcher(path: &Path, script: &str) -> std::io::Result<()> {
    match std::fs::read_to_string(path) {
        Ok(existing) if existing == script => return Ok(()),
        Ok(existing) if !existing.contains(MARKER) => return Err(std::io::Error::other("a t3 launcher the app did not write")),
        _ => {}
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let staging = path.with_extension(format!("{}.tmp", std::process::id()));
    std::fs::write(&staging, script)?;
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755))?;
    std::fs::rename(&staging, path)
}

/// A launcher an app wrote; never reads a large binary another `t3` is.
fn has_marker(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() <= 16_384) && std::fs::read_to_string(path).is_ok_and(|c| c.contains(MARKER))
}

fn is_our_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink()) && has_marker(path)
}

fn is_executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

struct Paths {
    launcher: PathBuf,
    candidates: Vec<PathBuf>,
    path_env: String,
}

impl Paths {
    fn path_dirs(&self) -> Vec<PathBuf> {
        self.path_env.split(':').filter(|d| !d.is_empty()).map(PathBuf::from).collect()
    }

    fn first_on_path(&self) -> Option<PathBuf> {
        self.path_dirs().into_iter().map(|d| d.join("t3")).find(|c| is_executable(c))
    }

    fn runs_ours(&self, candidate: &Path) -> bool {
        let same = matches!((candidate.canonicalize(), self.launcher.canonicalize()), (Ok(a), Ok(b)) if a == b);
        is_our_link(candidate) || (same && has_marker(candidate))
    }

    fn status(&self) -> Status {
        let first = self.first_on_path();
        Status {
            installed: self.candidates.iter().map(|d| d.join("t3")).find(|l| is_our_link(l)),
            on_path: first.as_deref().is_some_and(|f| self.runs_ours(f)),
            shadowed_by: first.filter(|f| !self.runs_ours(f)),
        }
    }

    fn install(&self, script: &str) -> Result<(), String> {
        let launcher = self.launcher.display();
        write_launcher(&self.launcher, script).map_err(|_| format!("Could not set up the t3 launcher at {launcher}."))?;
        if let Some(link) = self.status().installed {
            if std::fs::read_link(&link).is_ok_and(|target| target == self.launcher) {
                return Ok(());
            }
            std::fs::remove_file(&link).map_err(|_| format!("Could not replace {}.", link.display()))?;
        }
        if let Some(other) = self.status().shadowed_by {
            return Err(format!("Another t3 at {} runs first in a new terminal. Remove it, or run the launcher directly at {launcher}.", other.display()));
        }
        let on_path = self.path_dirs();
        let (first, rest): (Vec<&PathBuf>, Vec<&PathBuf>) = self.candidates.iter().partition(|d| on_path.contains(*d));
        for dir in first.into_iter().chain(rest) {
            if std::fs::create_dir_all(dir).is_ok() && std::os::unix::fs::symlink(&self.launcher, dir.join("t3")).is_ok() {
                return Ok(());
            }
        }
        Err(format!("Another t3 command is already installed, or no folder on your PATH is writable. Run the launcher directly at {launcher}."))
    }

    fn uninstall(&self) -> Result<(), String> {
        for link in self.candidates.iter().map(|d| d.join("t3")).filter(|l| is_our_link(l)) {
            std::fs::remove_file(&link).map_err(|_| format!("Could not remove {}.", link.display()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_runs_the_bundled_server_with_quoted_paths() {
        assert_eq!(shell_word("/a/it's"), r#"'/a/it'"'"'s'"#);
        let script = launcher_script(Path::new("/h/.s5code/bin/t3"), Path::new("/h/.s5code"), Path::new("/Apps/S5 Code.app/s5code-server"));
        assert!(script.starts_with("#!/bin/sh\n") && script.contains(MARKER));
        assert!(script.contains("app='/Apps/S5 Code.app/s5code-server'\n"));
        assert!(script.ends_with("exec \"$app\" \"$@\"\n"));
    }

    #[test]
    fn installs_a_t3_link_on_path_and_removes_only_its_own() {
        let root = std::env::temp_dir().join(format!("s5-cli-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (local, foreign) = (root.join("home/.local/bin"), root.join("usr/bin"));
        std::fs::create_dir_all(&foreign).unwrap();
        let paths = Paths {
            launcher: root.join("t3home/bin/t3"),
            candidates: vec![root.join("home/bin"), local.clone()],
            path_env: format!("{}:{}", local.display(), foreign.display()),
        };
        let script = launcher_script(&paths.launcher, &root.join("t3home"), Path::new("/bin/true"));
        assert_eq!(paths.status(), Status { installed: None, on_path: false, shadowed_by: None });

        paths.install(&script).unwrap();
        let link = local.join("t3");
        assert_eq!(paths.status(), Status { installed: Some(link.clone()), on_path: true, shadowed_by: None }, "the folder on PATH wins");
        assert!(is_executable(&paths.launcher));
        paths.install(&script).unwrap();

        paths.uninstall().unwrap();
        assert!(!link.exists() && paths.launcher.exists());

        // Another t3 earlier on PATH: install refuses, and uninstall leaves it alone.
        let paths = Paths { path_env: format!("{}:{}", foreign.display(), local.display()), ..paths };
        std::fs::write(foreign.join("t3"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(foreign.join("t3"), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(paths.status().shadowed_by, Some(foreign.join("t3")));
        assert!(paths.install(&script).unwrap_err().contains("runs first"));
        paths.uninstall().unwrap();
        assert!(foreign.join("t3").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
