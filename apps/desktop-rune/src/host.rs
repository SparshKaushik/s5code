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
    /// The server keeps exiting before it is ready; the supervisor still retries.
    Failing(StartupFailure),
}

/// Electron's `BackendStartupFailure`, for the startup recovery screen.
#[derive(Clone, Debug, PartialEq)]
pub struct StartupFailure {
    pub reason: String,
    /// The tail of what the failed attempt wrote.
    pub output: String,
    pub log: PathBuf,
}

const FIRST_PORT: u16 = 3773;
const READINESS_PATH: &str = "/.well-known/t3/environment";
const READINESS_TIMEOUT: Duration = Duration::from_secs(180);
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(15);
const TERMINATE_GRACE: Duration = Duration::from_secs(5);
/// contracts' SERVER_EXIT_CODE_STATE_DIR_OWNED and SERVER_EXIT_CODE_MANAGED_POLICY: never restart.
const EXIT_STATE_DIR_OWNED: i32 = 78;
const EXIT_MANAGED_POLICY: i32 = 77;
/// Electron's STARTUP_FAILURE_RESTART_ATTEMPTS and STARTUP_FAILURE_OUTPUT_CHARS.
const STARTUP_FAILURE_ATTEMPTS: u32 = 3;
const STARTUP_FAILURE_OUTPUT_CHARS: usize = 4_000;

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
        let (serve, serve_port) = crate::desktop::exposure::tailscale_serve();
        let bootstrap = serde_json::json!({
            "mode": "desktop",
            "noBrowser": true,
            "port": p,
            "t3Home": home.to_string_lossy(),
            "host": crate::desktop::exposure::bind_host(),
            "desktopBootstrapToken": credential,
            "tailscaleServeEnabled": serve,
            "tailscaleServePort": serve_port,
        });
        let output_from = std::fs::metadata(&log_path).map_or(0, |m| m.len());
        let child = match spawn(&binary, &log_path, &bootstrap.to_string()) {
            Ok(c) => c,
            Err(e) => return send(Event::Failed(format!("Could not start {}: {e}", binary.display()))),
        };
        *sup.child.lock().unwrap() = Some(child);

        let started = Instant::now();
        let mut ready = false;
        let mut timed_out = false;
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
                    timed_out = true;
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
        if !ready && (timed_out || attempt + 1 >= STARTUP_FAILURE_ATTEMPTS) {
            let reason = if timed_out { "The server started but did not become ready in time.".to_owned() } else { format!("The server could not start ({}).", exit_reason(exit)) };
            send(Event::Failing(StartupFailure { reason, output: output_tail(&log_path, output_from), log: log_path.clone() }));
        }
        std::thread::sleep(Duration::from_millis(500).saturating_mul(1 << attempt.min(5)).min(Duration::from_secs(10)));
        attempt += 1;
    }
}

/// Electron's exit reason: `code=N`, or `killed by SIGNAL`.
fn exit_reason(status: Option<std::process::ExitStatus>) -> String {
    let Some(status) = status else { return "code=null".into() };
    if let Some(code) = status.code() {
        return format!("code={code}");
    }
    #[cfg(unix)]
    if let Some(signal) = std::os::unix::process::ExitStatusExt::signal(&status) {
        let name = match signal {
            6 => "SIGABRT".to_owned(),
            9 => "SIGKILL".to_owned(),
            11 => "SIGSEGV".to_owned(),
            15 => "SIGTERM".to_owned(),
            n => format!("signal {n}"),
        };
        return format!("killed by {name}");
    }
    "code=null".into()
}

/// What the log gained since `from`, its last `STARTUP_FAILURE_OUTPUT_CHARS` characters.
fn output_tail(log_path: &PathBuf, from: u64) -> String {
    let bytes = std::fs::read(log_path).unwrap_or_default();
    let text = String::from_utf8_lossy(bytes.get(from as usize..).unwrap_or_default());
    tail_chars(&text, STARTUP_FAILURE_OUTPUT_CHARS).to_owned()
}

fn tail_chars(text: &str, n: usize) -> &str {
    let count = text.chars().count();
    let skip = count.saturating_sub(n);
    text.char_indices().nth(skip).map_or("", |(i, _)| &text[i..])
}

/// Whether the server is gone, with its status when known. `terminate` (a restart) takes the
/// child it waited for, so an empty slot is an exit too.
fn exited(child: &mut Option<Child>) -> Option<Option<std::process::ExitStatus>> {
    match child {
        None => Some(None),
        Some(c) => c.try_wait().ok().flatten().map(Some),
    }
}

fn spawn(binary: &PathBuf, log_path: &PathBuf, bootstrap: &str) -> std::io::Result<Child> {
    let out = File::options().create(true).append(true).open(log_path)?;
    let mut child = Command::new(binary)
        .args(["--bootstrap-fd", "0"])
        .stdin(Stdio::piped())
        .stdout(out.try_clone()?)
        .stderr(out)
        .spawn()?;
    // One JSON line; the server reads fd 0 once and the pipe closes on drop.
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(format!("{bootstrap}\n").as_bytes())?;
    Ok(child)
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
        Ok(Event::Failing(f)) => {
            eprintln!("check-server: {}\n{}", f.reason, f.output);
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
        assert_eq!(exit_reason(done.take().unwrap().wait().ok()), "killed by SIGKILL");
    }

    #[test]
    fn a_failed_start_reports_its_exit_and_the_output_it_wrote() {
        let status = Command::new("sh").args(["-c", "exit 3"]).status().unwrap();
        assert_eq!(exit_reason(Some(status)), "code=3");
        assert_eq!(exit_reason(None), "code=null");
        assert_eq!(tail_chars("abcdé", 2), "dé");
        assert_eq!(tail_chars("ab", 4000), "ab");
        assert_eq!(tail_chars("", 10), "");
        let path = std::env::temp_dir().join(format!("s5-host-tail-{}.log", std::process::id()));
        std::fs::write(&path, "first launch\nsecond: boom\n").unwrap();
        assert_eq!(output_tail(&path, "first launch\n".len() as u64), "second: boom\n");
        assert_eq!(output_tail(&path, 999), "");
        let _ = std::fs::remove_file(&path);
    }
}
