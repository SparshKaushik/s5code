//! The login shell's PATH, as the server's `fixPath` and Electron's DesktopShellEnvironment read
//! it: an app opened from Finder or a launcher inherits a minimal PATH that misses `~/.local/bin`,
//! Homebrew and whatever the user's profile adds. Read once, off the UI thread.

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const START: &str = "__T3CODE_ENV_PATH_START__";
const END: &str = "__T3CODE_ENV_PATH_END__";
const TIMEOUT: Duration = Duration::from_secs(5);

static PATH: OnceLock<String> = OnceLock::new();

/// Starts reading the login shell's PATH, waking the app when it lands.
pub fn warm(wake: Option<rune::Waker>) {
    if PATH.get().is_some() {
        return;
    }
    std::thread::spawn(move || {
        let _ = PATH.set(resolve());
        if let Some(wake) = wake {
            wake();
        }
    });
}

/// The merged PATH once read; until then the app's own.
pub fn get() -> Option<&'static str> {
    PATH.get().map(String::as_str)
}

pub fn current() -> String {
    get().map(str::to_owned).unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
}

fn resolve() -> String {
    let inherited = std::env::var("PATH").ok();
    let shell_path = candidates(std::env::var("SHELL").ok().as_deref()).into_iter().find_map(|shell| read(&shell));
    let launchctl = if cfg!(target_os = "macos") && shell_path.is_none() { launchctl_path() } else { None };
    merge(shell_path.or(launchctl).as_deref(), inherited.as_deref())
}

/// `listLoginShellCandidates`: `$SHELL`, then the platform's default shell.
fn candidates(shell: Option<&str>) -> Vec<String> {
    let fallback = if cfg!(target_os = "macos") { "/bin/zsh" } else { "/bin/bash" };
    let mut out: Vec<String> = Vec::new();
    for c in [shell.map(str::trim).filter(|s| !s.is_empty()), Some(fallback)].into_iter().flatten() {
        if !out.iter().any(|o| o == c) {
            out.push(c.to_owned());
        }
    }
    out
}

fn capture_command(shell: &str) -> String {
    let name = std::path::Path::new(shell).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name.starts_with("nu") {
        format!("print '{START}'; print ($env.PATH? | default '' | if ($in | describe | str starts-with 'list') {{ $in | str join (char esep) }} else {{ $in }}); print '{END}'")
    } else {
        format!("printf '%s\\n' '{START}'; printenv PATH || true; printf '%s\\n' '{END}'")
    }
}

/// `readEnvironmentFromLoginShell`: an interactive login shell, detached from any terminal.
fn read(shell: &str) -> Option<String> {
    let mut child = Command::new(shell)
        .args(["-ilc", &capture_command(shell)])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    extract(&out)
}

fn extract(output: &str) -> Option<String> {
    let start = output.find(START)? + START.len();
    let end = start + output[start..].find(END)?;
    let value = output[start..end].trim_start_matches(['\r', '\n']).trim_end_matches(['\r', '\n']);
    (!value.is_empty()).then(|| value.to_owned())
}

fn launchctl_path() -> Option<String> {
    let out = Command::new("/bin/launchctl").args(["getenv", "PATH"]).stderr(Stdio::null()).output().ok()?;
    let path = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!path.is_empty()).then_some(path)
}

/// `mergePathEntries`: the shell's entries first, then the inherited ones it lacks.
fn merge(preferred: Option<&str>, inherited: Option<&str>) -> String {
    let mut out: Vec<&str> = Vec::new();
    for entry in [preferred, inherited].into_iter().flatten().flat_map(|p| p.split(':')).map(str::trim) {
        if !entry.is_empty() && !out.contains(&entry) {
            out.push(entry);
        }
    }
    out.join(":")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_login_shell_path_between_markers_and_merges_it_first() {
        let noisy = format!("motd\n{START}\n/opt/homebrew/bin:/usr/bin\n{END}\nbye");
        assert_eq!(extract(&noisy).as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(extract(&format!("{START}\n\n{END}")), None);
        assert_eq!(extract("no markers"), None);
        assert_eq!(merge(Some("/opt/homebrew/bin:/usr/bin"), Some("/usr/bin:/bin: ")), "/opt/homebrew/bin:/usr/bin:/bin");
        assert_eq!(merge(None, Some("/usr/bin")), "/usr/bin");
        assert_eq!(candidates(Some(" /bin/bash ")).first().map(String::as_str), Some("/bin/bash"));
        assert!(capture_command("/usr/bin/fish").contains("printenv PATH"));
        assert!(capture_command("/opt/nu").starts_with("print '"));
    }
}
