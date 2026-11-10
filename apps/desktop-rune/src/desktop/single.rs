//! One app per T3 home, as Electron's `requestSingleInstanceLock`: a second launch hands its
//! activation token and any prompt links on its command line (`second-instance` argv) to the
//! running app, which focuses its window and opens them, and exits.
//! Unix only: a socket in the T3 home's userdata directory is the lock.

use std::io::{BufRead, Write};

#[cfg(unix)]
pub use unix::claim;

/// What a second launch sends: the token on the first line, then one link per line.
fn write_handoff(mut w: impl Write, token: &str, links: &[String]) -> std::io::Result<()> {
    writeln!(w, "{}", token.trim())?;
    for link in links {
        // URL parsing drops line breaks anyway, so stripping them loses nothing.
        writeln!(w, "{}", link.replace(['\r', '\n'], ""))?;
    }
    w.flush()
}

fn read_handoff(r: impl BufRead) -> (String, Vec<String>) {
    let mut lines = r.lines().map_while(Result::ok);
    let token = lines.next().unwrap_or_default().trim().to_owned();
    (token, lines.filter(|l| !l.trim().is_empty()).collect())
}

/// This launch's prompt links, for the app that already runs.
fn argv_links() -> Vec<String> {
    std::env::args().skip(1).filter(|a| crate::links::parse(a).is_some()).collect()
}

fn deliver(app: &rune::AppHandle<crate::App>, r: impl BufRead) {
    let (token, links) = read_handoff(r);
    app.activate(token);
    for link in links {
        app.update(move |s, tx| {
            crate::links::on_open_url(s, &link, tx);
            crate::links::poll(s);
        });
    }
}

#[cfg(unix)]
mod unix {
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
                    Ok(running) => {
                        let token = ["XDG_ACTIVATION_TOKEN", "DESKTOP_STARTUP_ID"].iter().find_map(|k| std::env::var(k).ok()).unwrap_or_default();
                        let links = super::argv_links();
                        let _ = super::write_handoff(running, &token, &links);
                        eprintln!("s5code-rune: already running; handed it this launch ({} link(s))", links.len());
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
        /// Brings the window forward and opens the links of each later launch.
        pub fn serve(&self, app: rune::AppHandle<crate::App>) {
            let Some(listener) = self.lock.as_ref().and_then(|(_, l)| l.try_clone().ok()) else { return };
            std::thread::spawn(move || {
                for stream in listener.incoming().flatten() {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    super::deliver(&app, std::io::BufReader::new(stream));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_launch_hands_over_its_token_and_links() {
        let links = vec!["s5code://new?prompt=Fix%20it".to_owned(), "https://app.t3.codes/new?prompt=a\nb".to_owned()];
        let mut wire = Vec::new();
        write_handoff(&mut wire, " token-1\n", &links).unwrap();
        let (token, got) = read_handoff(wire.as_slice());
        assert_eq!(token, "token-1");
        assert_eq!(got, ["s5code://new?prompt=Fix%20it", "https://app.t3.codes/new?prompt=ab"]);
        let (token, got) = read_handoff(&b"\n"[..]);
        assert_eq!((token.as_str(), got.len()), ("", 0), "a launch without a token or links still focuses");
    }
}
