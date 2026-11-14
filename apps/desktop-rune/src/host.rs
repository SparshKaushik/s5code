//! Which server this window talks to, mirroring the Electron shell's backend manager
//! (apps/desktop/src/backend): `S5_SERVER_URL` + `S5_TOKEN` point at a running server; otherwise
//! the bundled `s5code-server` binary is started on a free loopback port with a desktop bootstrap
//! envelope, supervised, and stopped on quit. Headless renders and a build without the binary
//! fall back to fixtures.

use std::fs::File;
use std::io::Write;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub enum Event {
    /// Connect (or reconnect, if the port changed after a restart) with a bearer token.
    Ready { url: String, token: String },
    Failed(String),
}

const FIRST_PORT: u16 = 3773;
const READINESS_PATH: &str = "/.well-known/t3/environment";
const READINESS_TIMEOUT: Duration = Duration::from_secs(180);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(15);
const TERMINATE_GRACE: Duration = Duration::from_secs(5);
/// contracts' SERVER_EXIT_CODE_STATE_DIR_OWNED and SERVER_EXIT_CODE_MANAGED_POLICY: never restart.
const EXIT_STATE_DIR_OWNED: i32 = 78;
const EXIT_MANAGED_POLICY: i32 = 77;

struct Supervisor {
    child: Mutex<Option<Child>>,
    stopping: AtomicBool,
    /// The next exit is `restart`'s: start again at once.
    restarting: AtomicBool,
}

static SUPERVISOR: Mutex<Option<Arc<Supervisor>>> = Mutex::new(None);

fn remote() -> Option<(String, String)> {
    let url = std::env::var("S5_SERVER_URL").ok()?;
    let token = std::env::var("S5_TOKEN").ok().or_else(|| {
        let path = std::env::var("S5_TOKEN_FILE").ok()?;
        std::fs::read_to_string(path).ok().map(|t| t.trim().to_string())
    })?;
    Some((url, token))
}

/// `s5code-server` beside the executable (`Contents/MacOS` in the macOS bundle).
pub(crate) fn bundled_binary() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("S5_SERVER_BIN") {
        return Some(PathBuf::from(path));
    }
    let path = std::env::current_exe().ok()?.with_file_name("s5code-server");
    path.is_file().then_some(path)
}

/// Same base directory the Electron app and `npx t3` use, so both see one database.
pub(crate) fn t3_home() -> PathBuf {
    match std::env::var("T3CODE_HOME").map(|v| v.trim().to_string()) {
        Ok(v) if !v.is_empty() => PathBuf::from(v),
        _ => PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".s5code"),
    }
}

/// Whether the window should wait for a live server instead of showing fixtures.
pub fn live() -> bool {
    !rune::tooling::is_headless() && (remote().is_some() || bundled_binary().is_some())
}

pub fn start(wake: impl Fn() + Send + Sync + 'static) -> Option<Receiver<Event>> {
    if rune::tooling::is_headless() {
        return None;
    }
    launch(wake)
}

/// `start` without Rune's argument parsing, which would reject `--check-server`.
fn launch(wake: impl Fn() + Send + Sync + 'static) -> Option<Receiver<Event>> {
    let (tx, rx) = channel();
    if let Some((url, token)) = remote() {
        let _ = tx.send(Event::Ready { url, token });
        return Some(rx);
    }
    let binary = bundled_binary()?;
    let sup = Arc::new(Supervisor { child: Mutex::new(None), stopping: AtomicBool::new(false), restarting: AtomicBool::new(false) });
    *SUPERVISOR.lock().unwrap() = Some(sup.clone());
    std::thread::spawn(move || supervise(binary, sup, tx, wake));
    Some(rx)
}

/// Free on every address the server might bind, like the Electron app's scan from 3773.
fn port_free(port: u16) -> bool {
    ["127.0.0.1", "0.0.0.0"].iter().all(|host| TcpListener::bind((*host, port)).is_ok())
}

fn free_port() -> Option<u16> {
    (FIRST_PORT..u16::MAX).find(|p| port_free(*p))
}

fn supervise(binary: PathBuf, sup: Arc<Supervisor>, tx: Sender<Event>, wake: impl Fn()) {
    let send = |e: Event| {
        let _ = tx.send(e);
        wake();
    };
    let home = t3_home();
    let log_path = home.join("userdata/logs/desktop-rune-server.log");
    let _ = std::fs::create_dir_all(log_path.parent().unwrap());
    // One launch per log: restarts append, the next launch starts over.
    let _ = File::create(&log_path);
    // Reused across restarts so the server re-seeds the same grant and existing sessions resume.
    let credential = format!("{}{}", s5_net::uuid(), s5_net::uuid()).replace('-', "");
    let mut port: Option<u16> = None;
    let mut bearer: Option<String> = None;
    // A restart that had to move ports announces the new URL; one on the same port lets Net resume.
    let mut announced: Option<u16> = None;
    let mut attempt = 0u32;
    while !sup.stopping.load(Ordering::SeqCst) {
        let reuse = port.filter(|p| port_free(*p));
        let Some(p) = reuse.or_else(free_port) else {
            return send(Event::Failed("No free port for the S5 Code server.".into()));
        };
        port = Some(p);
        let base = format!("http://127.0.0.1:{p}");
        let bootstrap = serde_json::json!({
            "mode": "desktop",
            "noBrowser": true,
            "port": p,
            "t3Home": home.to_string_lossy(),
            "host": crate::desktop::exposure::bind_host(),
            "desktopBootstrapToken": credential,
            "tailscaleServeEnabled": false,
            "tailscaleServePort": 443,
            "desktopBrowserFd": BROWSER_EVENTS_FD,
            "desktopBrowserControlFd": BROWSER_CONTROL_FD,
        });
        let child = match spawn(&binary, &log_path, &bootstrap.to_string()) {
            Ok(c) => c,
            Err(e) => return send(Event::Failed(format!("Could not start {}: {e}", binary.display()))),
        };
        *sup.child.lock().unwrap() = Some(child);

        let started = Instant::now();
        let mut ready = false;
        let exit = loop {
            if let Some(status) = exited(&mut sup.child.lock().unwrap()) {
                break status;
            }
            if sup.stopping.load(Ordering::SeqCst) {
                return;
            }
            if !ready {
                if probe(&base) {
                    ready = true;
                    attempt = 0;
                    if bearer.is_none() {
                        match exchange(&base, &credential) {
                            Ok(t) => bearer = Some(t),
                            Err(e) => {
                                terminate(&sup);
                                return send(Event::Failed(format!("The S5 Code server refused the desktop credential: {e}")));
                            }
                        }
                    }
                    if announced != Some(p) {
                        announced = Some(p);
                        send(Event::Ready { url: base.clone(), token: bearer.clone().unwrap() });
                    }
                } else if started.elapsed() > READINESS_TIMEOUT {
                    terminate(&sup);
                    break None;
                }
            }
            std::thread::sleep(Duration::from_millis(if ready { 500 } else { 100 }));
        };
        sup.child.lock().unwrap().take();
        if sup.stopping.load(Ordering::SeqCst) {
            return;
        }
        if sup.restarting.swap(false, Ordering::SeqCst) {
            log(&log_path, "restarting for a settings change");
            attempt = 0;
            continue;
        }
        let code = exit.and_then(|s| s.code());
        if code == Some(EXIT_STATE_DIR_OWNED) {
            return send(Event::Failed(format!(
                "Another S5 Code server already owns {}. Quit it (or the Electron app), then reopen this one.",
                home.display()
            )));
        }
        if code == Some(EXIT_MANAGED_POLICY) {
            return send(Event::Failed("Your organization's policy refused to start the S5 Code server.".into()));
        }
        log(&log_path, &format!("server exited ({code:?}); restarting"));
        std::thread::sleep(Duration::from_millis(500).saturating_mul(1 << attempt.min(5)).min(Duration::from_secs(10)));
        attempt += 1;
    }
}

/// Whether the server is gone, with its status when known. `terminate` (a restart) takes the
/// child it waited for, so an empty slot is an exit too.
fn exited(child: &mut Option<Child>) -> Option<Option<std::process::ExitStatus>> {
    match child {
        None => Some(None),
        Some(c) => c.try_wait().ok().flatten().map(Some),
    }
}

/// The server reads browser events from one and writes browser commands to the other, as the
/// Electron shell's backend manager hands them over (`DesktopBrowserChannel`).
const BROWSER_EVENTS_FD: i32 = 6;
const BROWSER_CONTROL_FD: i32 = 7;

fn spawn(binary: &PathBuf, log_path: &PathBuf, bootstrap: &str) -> std::io::Result<Child> {
    use std::os::fd::AsRawFd;
    use std::os::unix::process::CommandExt;
    let out = File::options().create(true).append(true).open(log_path)?;
    let (events, server_events) = std::os::unix::net::UnixStream::pair()?;
    let (commands, server_commands) = std::os::unix::net::UnixStream::pair()?;
    let fds = [(server_events.as_raw_fd(), BROWSER_EVENTS_FD), (server_commands.as_raw_fd(), BROWSER_CONTROL_FD)];
    let mut command = Command::new(binary);
    command.args(["--bootstrap-fd", "0"]).stdin(Stdio::piped()).stdout(out.try_clone()?).stderr(out);
    // SAFETY: between fork and exec this only calls fcntl and dup2, which are async-signal-safe.
    unsafe {
        command.pre_exec(move || {
            // Copies above both targets first, so placing one never closes the other's source;
            // the copies and their targets lose close-on-exec.
            let mut high = [0; 2];
            for (i, (fd, _)) in fds.iter().enumerate() {
                high[i] = fcntl(*fd, F_DUPFD, 10);
                if high[i] < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            for (i, (_, target)) in fds.iter().enumerate() {
                if dup2(high[i], *target) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    drop((server_events, server_commands));
    crate::panel::agent::connect(events, commands);
    // One JSON line; the server reads fd 0 once and the pipe closes on drop.
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(format!("{bootstrap}\n").as_bytes())?;
    Ok(child)
}

const F_DUPFD: i32 = 0;
unsafe extern "C" {
    fn fcntl(fd: i32, cmd: i32, ...) -> i32;
    fn dup2(from: i32, to: i32) -> i32;
}

fn log(path: &PathBuf, line: &str) {
    if let Ok(mut f) = File::options().create(true).append(true).open(path) {
        let _ = writeln!(f, "[desktop-rune] {line}");
    }
}

fn probe(base: &str) -> bool {
    ureq::get(&format!("{base}{READINESS_PATH}")).timeout(Duration::from_secs(2)).call().is_ok()
}

fn exchange(base: &str, credential: &str) -> Result<String, String> {
    let started = Instant::now();
    loop {
        match s5_net::exchange_bootstrap(base, credential) {
            Ok(token) => return Ok(token),
            Err(e) if started.elapsed() > EXCHANGE_TIMEOUT => return Err(e),
            Err(_) => std::thread::sleep(Duration::from_millis(500)),
        }
    }
}

/// SIGTERM, then a kill after the grace period, like the Electron backend manager.
fn terminate(sup: &Supervisor) {
    let mut guard = sup.child.lock().unwrap();
    let Some(child) = guard.as_mut() else { return };
    let _ = Command::new("kill").args(["-TERM", &child.id().to_string()]).status();
    let deadline = Instant::now() + TERMINATE_GRACE;
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            guard.take();
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    guard.take();
}

/// Whether this window runs the bundled server, so its settings apply.
pub fn managed() -> bool {
    SUPERVISOR.lock().unwrap().is_some()
}

/// Restart the bundled server on its port, as Electron's relaunch after an exposure change.
/// Clients resume their sessions once it answers again.
pub fn restart() {
    let Some(sup) = SUPERVISOR.lock().unwrap().clone() else { return };
    sup.restarting.store(true, Ordering::SeqCst);
    std::thread::spawn(move || terminate(&sup));
}

pub fn shutdown() {
    if let Some(sup) = SUPERVISOR.lock().unwrap().take() {
        sup.stopping.store(true, Ordering::SeqCst);
        terminate(&sup);
    }
}

/// `s5code-rune --check-server`: start the bundled server, sign in, load the shell, stop it.
/// CI runs this against the packaged app; exits non-zero with the reason on failure.
pub fn check() -> i32 {
    let rx = match launch(|| {}) {
        Some(rx) => rx,
        None => {
            eprintln!("check-server: no bundled server binary or S5_SERVER_URL");
            return 1;
        }
    };
    let code = match rx.recv_timeout(READINESS_TIMEOUT + EXCHANGE_TIMEOUT) {
        Ok(Event::Ready { url, token }) => {
            let mut net = s5_net::Net::connect(&url, &token, || {});
            let deadline = Instant::now() + Duration::from_secs(60);
            while !net.state().shell_ready && Instant::now() < deadline {
                net.drain();
                std::thread::sleep(Duration::from_millis(100));
            }
            let st = net.state();
            println!("check-server: {url} conn={:?} shell_ready={} projects={} threads={}", st.conn, st.shell_ready, st.projects.len(), st.threads.len());
            if st.shell_ready { 0 } else { 1 }
        }
        Ok(Event::Failed(e)) => {
            eprintln!("check-server: {e}");
            1
        }
        Err(_) => {
            eprintln!("check-server: timed out waiting for the server");
            1
        }
    };
    shutdown();
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_server_restart_took_counts_as_exited() {
        assert_eq!(exited(&mut None), Some(None));
        let mut running = Some(Command::new("sleep").arg("5").spawn().unwrap());
        assert_eq!(exited(&mut running), None);
        let mut child = running.take().unwrap();
        child.kill().unwrap();
        child.wait().unwrap();
        let mut done = Some(child);
        assert!(matches!(exited(&mut done), Some(Some(_))));
    }
}
