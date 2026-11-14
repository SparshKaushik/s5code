//! The login shell's environment, as Electron's DesktopShellEnvironment reads it before it starts
//! the server: an app opened from Finder or a launcher inherits a minimal environment that misses
//! `~/.local/bin`, Homebrew, the SSH agent, CA bundles and whatever the user's profile adds.
//! Read once, off the UI thread.

use std::collections::HashMap;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const TIMEOUT: Duration = Duration::from_secs(5);
/// Electron's `FALLBACK_LC_CTYPE`: launchd sets no locale, and pbcopy then decodes as MacRoman.
const FALLBACK_LC_CTYPE: &str = "en_US.UTF-8";

/// Electron's `LOGIN_SHELL_ENV_NAMES`.
const NAMES: &[&str] = &[
    "PATH",
    "DBUS_SESSION_BUS_ADDRESS",
    "DISPLAY",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "SSH_AUTH_SOCK",
    "HOMEBREW_PREFIX",
    "HOMEBREW_CELLAR",
    "HOMEBREW_REPOSITORY",
    "XDG_CONFIG_HOME",
    "XDG_CURRENT_DESKTOP",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_DESKTOP",
    "XDG_SESSION_TYPE",
    "WAYLAND_DISPLAY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "T3CODE_TELEMETRY_ENABLED",
    "T3CODE_GITHUB_APP_CLIENT_ID",
    "__MISE_DIFF",
    "JAVA_HOME",
];
/// The shell's value wins over the inherited one.
const SHELL_PREFERRED: &[&str] = &["DBUS_SESSION_BUS_ADDRESS", "XDG_CURRENT_DESKTOP", "XDG_SESSION_DESKTOP", "XDG_SESSION_TYPE", "__MISE_DIFF"];
/// Filled from the shell only when the app has none.
const FILL_IN: &[&str] = &[
    "SSH_AUTH_SOCK",
    "DISPLAY",
    "HOMEBREW_PREFIX",
    "HOMEBREW_CELLAR",
    "HOMEBREW_REPOSITORY",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "WAYLAND_DISPLAY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    "JAVA_HOME",
    "T3CODE_TELEMETRY_ENABLED",
    "T3CODE_GITHUB_APP_CLIENT_ID",
];
const LOCALE: &[&str] = &["LANG", "LC_ALL", "LC_CTYPE"];

struct Shell {
    /// What the server should run with on top of the app's environment, PATH included.
    patch: Vec<(String, String)>,
    path: String,
    /// The shell answered with a PATH, so the server can skip its own probe.
    from_shell: bool,
}

static SHELL: OnceLock<Shell> = OnceLock::new();

fn load() -> &'static Shell {
    SHELL.get_or_init(resolve)
}

/// Starts reading the login shell's environment, waking the app when it lands.
pub fn warm(wake: Option<rune::Waker>) {
    if SHELL.get().is_some() {
        return;
    }
    std::thread::spawn(move || {
        load();
        if let Some(wake) = wake {
            wake();
        }
    });
}

/// The merged PATH once read; until then the app's own.
pub fn get() -> Option<&'static str> {
    SHELL.get().map(|s| s.path.as_str())
}

pub fn current() -> String {
    get().map(str::to_owned).unwrap_or_else(|| std::env::var("PATH").unwrap_or_default())
}

/// Electron's `installIntoProcess` for the server: the variables to set, and whether the shell
/// gave a PATH (`shellEnvironmentPrepared`). Blocks until the shell has answered or timed out.
pub fn server_env() -> (&'static [(String, String)], bool) {
    let shell = load();
    (&shell.patch, shell.from_shell)
}

fn resolve() -> Shell {
    let mut shell: HashMap<String, String> = HashMap::new();
    for candidate in candidates(std::env::var("SHELL").ok().as_deref()) {
        shell.extend(read(&candidate));
        if shell.contains_key("PATH") {
            break;
        }
    }
    let from_shell = shell.contains_key("PATH");
    if cfg!(target_os = "macos") && !from_shell
        && let Some(path) = launchctl_path()
    {
        shell.insert("PATH".into(), path);
    }
    let inherited: HashMap<String, String> = std::env::vars().collect();
    let uid = runtime_uid();
    let patch = patch(&shell, &inherited, cfg!(target_os = "macos"), uid, |path| std::path::Path::new(path).exists());
    let path = patch.iter().find(|(k, _)| k == "PATH").map(|(_, v)| v.clone()).unwrap_or_else(|| inherited.get("PATH").cloned().unwrap_or_default());
    Shell { patch, path, from_shell }
}

fn runtime_uid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    if cfg!(target_os = "linux") { std::fs::metadata("/proc/self").ok().map(|m| m.uid()) } else { None }
}

fn non_empty<'a>(env: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    env.get(name).map(|v| v.trim()).filter(|v| !v.is_empty())
}

/// `installPosixEnvironment`'s rules, as the changes to make to `inherited`.
fn patch(shell: &HashMap<String, String>, inherited: &HashMap<String, String>, macos: bool, uid: Option<u32>, exists: impl Fn(&str) -> bool) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let has = |out: &[(String, String)], name: &str| out.iter().any(|(k, v)| k == name && !v.is_empty()) || inherited.get(name).is_some_and(|v| !v.is_empty());
    let merged = merge(shell.get("PATH").map(String::as_str), inherited.get("PATH").map(String::as_str));
    if !merged.is_empty() {
        out.push(("PATH".into(), merged));
    }
    for name in SHELL_PREFERRED {
        if let Some(value) = shell.get(*name).filter(|v| !v.is_empty()) {
            out.push(((*name).into(), value.clone()));
        }
    }
    for name in FILL_IN {
        if !has(&out, name)
            && let Some(value) = shell.get(*name).filter(|v| !v.is_empty())
        {
            out.push(((*name).into(), value.clone()));
        }
    }
    // Locale variables move together; an inherited LC_ALL can override LANG or LC_CTYPE.
    if macos && LOCALE.iter().all(|name| non_empty(inherited, name).is_none()) {
        let from_shell: Vec<(String, String)> = LOCALE.iter().filter_map(|name| non_empty(shell, name).map(|v| ((*name).to_owned(), v.to_owned()))).collect();
        if from_shell.is_empty() {
            out.push(("LC_CTYPE".into(), FALLBACK_LC_CTYPE.into()));
        }
        out.extend(from_shell);
    }
    let bus_set = out.iter().any(|(k, v)| k == "DBUS_SESSION_BUS_ADDRESS" && !v.trim().is_empty()) || non_empty(inherited, "DBUS_SESSION_BUS_ADDRESS").is_some();
    if !macos && !bus_set {
        let runtime = out.iter().find(|(k, _)| k == "XDG_RUNTIME_DIR").map(|(_, v)| v.as_str()).or_else(|| non_empty(inherited, "XDG_RUNTIME_DIR"));
        let dirs = runtime.map(|d| d.trim().trim_end_matches('/').to_owned()).into_iter().chain(uid.map(|uid| format!("/run/user/{uid}")));
        if let Some(bus) = dirs.filter(|d| !d.is_empty()).map(|d| format!("{d}/bus")).find(|bus| exists(bus)) {
            out.push(("DBUS_SESSION_BUS_ADDRESS".into(), format!("unix:path={bus}")));
        }
    }
    out
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

fn start(name: &str) -> String {
    format!("__T3CODE_ENV_{name}_START__")
}

fn end(name: &str) -> String {
    format!("__T3CODE_ENV_{name}_END__")
}

/// `buildEnvironmentCaptureCommand`, each variable between its own markers.
fn capture_command(shell: &str) -> String {
    let name = std::path::Path::new(shell).file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    let nu = name.starts_with("nu");
    let parts = NAMES.iter().map(|var| {
        let (s, e) = (start(var), end(var));
        match (nu, *var) {
            (true, "PATH") => format!(
                "print '{s}'; print (if ($env.PATH? | describe | str starts-with 'list') {{ $env.PATH | each {{|p| if ($p | str starts-with '~') {{ $p | path expand --no-symlink }} else {{ $p }} }} | str join (char esep) }} else {{ $env.PATH? | default '' }}); print '{e}'"
            ),
            (true, var) => format!("print '{s}'; print ($env.{var}? | default ''); print '{e}'"),
            (false, var) => format!("printf '%s\\n' '{s}'; printenv {var} || true; printf '%s\\n' '{e}'"),
        }
    });
    parts.collect::<Vec<_>>().join("; ")
}

/// `readEnvironmentFromLoginShell`: an interactive login shell, detached from any terminal.
fn read(shell: &str) -> HashMap<String, String> {
    let Ok(mut child) = Command::new(shell)
        .args(["-ilc", &capture_command(shell)])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
    else {
        return HashMap::new();
    };
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        if let Some(mut pipe) = stdout {
            let _ = std::io::Read::read_to_string(&mut pipe, &mut out);
        }
        out
    });
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return HashMap::new();
            }
        }
    }
    let out = reader.join().unwrap_or_default();
    NAMES.iter().filter_map(|name| extract(&out, name).map(|v| ((*name).to_owned(), v))).collect()
}

fn extract(output: &str, name: &str) -> Option<String> {
    let (s, e) = (start(name), end(name));
    let from = output.find(&s)? + s.len();
    let to = from + output[from..].find(&e)?;
    let value = output[from..to].trim_start_matches(['\r', '\n']).trim_end_matches(['\r', '\n']);
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

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
    }

    #[test]
    fn reads_the_login_shell_path_between_markers_and_merges_it_first() {
        let noisy = format!("motd\n{}\n/opt/homebrew/bin:/usr/bin\n{}\n{}\n\n{}\nbye", start("PATH"), end("PATH"), start("LANG"), end("LANG"));
        assert_eq!(extract(&noisy, "PATH").as_deref(), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(extract(&noisy, "LANG"), None, "an unset variable");
        assert_eq!(extract("no markers", "PATH"), None);
        assert_eq!(merge(Some("/opt/homebrew/bin:/usr/bin"), Some("/usr/bin:/bin: ")), "/opt/homebrew/bin:/usr/bin:/bin");
        assert_eq!(merge(None, Some("/usr/bin")), "/usr/bin");
        assert_eq!(candidates(Some(" /bin/bash ")).first().map(String::as_str), Some("/bin/bash"));
        assert!(capture_command("/usr/bin/fish").contains("printenv SSH_AUTH_SOCK || true"));
        assert!(capture_command("/opt/nu").starts_with("print '__T3CODE_ENV_PATH_START__'"));
    }

    #[test]
    fn the_server_gets_the_shells_environment_by_electrons_rules() {
        let shell = env(&[
            ("PATH", "/opt/homebrew/bin:/usr/bin"),
            ("SSH_AUTH_SOCK", "/tmp/agent.sock"),
            ("HOMEBREW_PREFIX", "/opt/homebrew"),
            ("NODE_EXTRA_CA_CERTS", "/etc/corp.pem"),
            ("XDG_SESSION_TYPE", "wayland"),
            ("LANG", "de_DE.UTF-8"),
        ]);
        let finder = env(&[("PATH", "/usr/bin:/bin"), ("HOMEBREW_PREFIX", "/usr/local"), ("XDG_SESSION_TYPE", "x11")]);
        let got = patch(&shell, &finder, true, None, |_| false);
        let get = |name: &str| got.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str());
        assert_eq!(get("PATH"), Some("/opt/homebrew/bin:/usr/bin:/bin"));
        assert_eq!(get("SSH_AUTH_SOCK"), Some("/tmp/agent.sock"), "filled in");
        assert_eq!(get("NODE_EXTRA_CA_CERTS"), Some("/etc/corp.pem"));
        assert_eq!(get("HOMEBREW_PREFIX"), None, "the app's own value stays");
        assert_eq!(get("XDG_SESSION_TYPE"), Some("wayland"), "the shell's wins");
        assert_eq!(get("LANG"), Some("de_DE.UTF-8"), "macOS takes the shell's locale when it has none");

        let launchd = patch(&env(&[]), &env(&[("PATH", "/usr/bin")]), true, None, |_| false);
        assert!(launchd.contains(&("LC_CTYPE".into(), FALLBACK_LC_CTYPE.into())));
        let set = patch(&shell, &env(&[("LC_ALL", "C")]), true, None, |_| false);
        assert!(!set.iter().any(|(k, _)| LOCALE.contains(&k.as_str())), "an inherited locale is kept whole");

        let linux = patch(&env(&[]), &env(&[("PATH", "/usr/bin")]), false, Some(1000), |p| p == "/run/user/1000/bus");
        assert!(linux.contains(&("DBUS_SESSION_BUS_ADDRESS".into(), "unix:path=/run/user/1000/bus".into())));
        assert!(!linux.iter().any(|(k, _)| k == "LC_CTYPE"), "Linux keeps its locale");
        let runtime = patch(&env(&[("XDG_RUNTIME_DIR", "/run/me/")]), &env(&[]), false, Some(1000), |p| p == "/run/me/bus");
        assert!(runtime.contains(&("DBUS_SESSION_BUS_ADDRESS".into(), "unix:path=/run/me/bus".into())));
    }
}
