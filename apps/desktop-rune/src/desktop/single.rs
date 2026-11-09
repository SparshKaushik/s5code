//! One app per T3 home, as Electron's `requestSingleInstanceLock`: a second launch hands its
//! activation token to the running app, which shows and focuses its window, and exits.
//! Unix only: a socket in the T3 home's userdata directory is the lock.

#[cfg(unix)]
pub use unix::claim;

#[cfg(unix)]
mod unix {
    use std::io::{BufRead, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    pub struct Instance {
        lock: Option<(PathBuf, UnixListener)>,
    }

    /// None when another app already runs here and took this launch.
    pub fn claim() -> Option<Instance> {
        let dir = crate::host::t3_home().join("userdata");
        if rune::is_headless() || std::fs::create_dir_all(&dir).is_err() {
            return Some(Instance { lock: None });
        }
        let path = dir.join("desktop-rune.sock");
        for _ in 0..2 {
            match UnixListener::bind(&path) {
                Ok(listener) => return Some(Instance { lock: Some((path, listener)) }),
                Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => match UnixStream::connect(&path) {
                    Ok(mut running) => {
                        let token = ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"].iter().find_map(|k| std::env::var(k).ok()).unwrap_or_default();
                        let _ = writeln!(running, "{}", token.trim());
                        eprintln!("s5code-rune: already running; focused that window");
                        return None;
                    }
                    // Nobody listens: a crashed app left it behind.
                    Err(_) => {
                        let _ = std::fs::remove_file(&path);
                    }
                },
                Err(_) => break,
            }
        }
        Some(Instance { lock: None })
    }

    impl Instance {
        /// Brings the window forward for each later launch.
        pub fn serve(&self, app: rune::AppHandle<crate::App>) {
            let Some(listener) = self.lock.as_ref().and_then(|(_, l)| l.try_clone().ok()) else { return };
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let mut token = String::new();
                    let _ = std::io::BufReader::new(stream).read_line(&mut token);
                    app.activate(token.trim().to_string());
                }
            });
        }
    }

    impl Drop for Instance {
        fn drop(&mut self) {
            if let Some((path, _)) = &self.lock {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

#[cfg(not(unix))]
pub struct Instance;

#[cfg(not(unix))]
pub fn claim() -> Option<Instance> {
    Some(Instance)
}

#[cfg(not(unix))]
impl Instance {
    pub fn serve(&self, _app: rune::AppHandle<crate::App>) {}
}
