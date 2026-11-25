//! Electron's DesktopUpdates for Rune builds. Electron reads electron-builder's `latest-mac.yml`
//! from this fork's GitHub releases; Rune builds publish their own `rune-v<version>` releases
//! (`.github/workflows/rune-release.yml`) with a `rune-update.json` manifest naming each
//! platform's archive and its SHA-256. The app checks 15 seconds after launch and every 4
//! minutes, downloads on request, verifies the archive, and installs it on restart: macOS swaps
//! the `.app` bundle (as `MacUnsignedUpdateInstall`), Linux the binaries beside the executable.

use std::ffi::OsStr;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rune::{Cursor, El, Tx, el, icon, row, text};
use serde_json::Value;

use crate::theme::{self, T, radius};
use crate::{App, icons as i, kit};

const REPO: &str = "SparshKaushik/s5code";
const TAG_PREFIX: &str = "rune-v";
const MANIFEST: &str = "rune-update.json";
const STARTUP_DELAY_MS: i64 = 15_000;
const POLL_MS: i64 = 4 * 60_000;
const BUNDLE_ID: &str = super::APP_ID;

/// The version the release workflow stamped into this build; `None` for a local or CI build,
/// which (like an Electron build without `app-update.yml`) doesn't update itself.
fn stamped() -> Option<&'static str> {
    option_env!("S5_RUNE_VERSION").filter(|v| !v.is_empty())
}

/// The GitHub release page of a Rune `version`.
pub fn release_page(version: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{TAG_PREFIX}{version}")
}

pub fn version() -> &'static str {
    stamped().unwrap_or(env!("CARGO_PKG_VERSION"))
}

/// The manifest key of this build's archive.
fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x64")
    } else {
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Context {
    Check,
    Download,
    Install,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading(f32),
    Downloaded,
    Installing,
    Error(Context, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub name: String,
    pub sha256: String,
    pub size: u64,
}

enum Event {
    Checked(Result<Option<Release>, String>),
    Progress(f32),
    Downloaded(Result<PathBuf, String>),
    Prepared(Result<PathBuf, String>),
}

#[derive(Default)]
pub struct Ui {
    pub status: Status,
    pub release: Option<Release>,
    staged: Option<PathBuf>,
    events: Arc<Mutex<Vec<Event>>>,
    next_check: Option<i64>,
    /// The menu asked: its outcome shows in an alert, as Electron's `checkForUpdatesFromMenu`.
    announce: bool,
    pub confirm: bool,
    /// The release a track switch checked (`setUpdateChannel`'s `minimumVersion`): until the app
    /// restarts, nothing older is offered, and only through it does a build cross tracks.
    floor: Option<String>,
    /// A scene's recorded state stands in for a stamped release build.
    fixture: bool,
}

/// Web `resolveDesktopUpdateButtonAction`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Download,
    Install,
}

pub fn action(ui: &Ui) -> Action {
    match (&ui.status, ui.staged.is_some(), ui.release.is_some()) {
        (Status::Downloaded, true, _) | (Status::Error(Context::Install, _), true, _) => Action::Install,
        (Status::Available, _, true) | (Status::Error(Context::Download, _), _, true) => Action::Download,
        _ => Action::None,
    }
}

/// Why this app can't update itself, if it can't (web `getDesktopUpdateDisabledReason`).
pub fn disabled(s: &App) -> Option<String> {
    if s.ui.desktop.updates.fixture {
        return None;
    }
    if let Some(denial) = crate::settings::about::managed_denial(s) {
        return Some(denial);
    }
    if stamped().is_none() {
        return Some(super::NO_UPDATE_FEED.to_owned());
    }
    if platform().is_none() {
        return Some("S5 Code (Rune) doesn't publish builds for this platform yet.".to_owned());
    }
    None
}

fn channel() -> String {
    super::prefs().get::<String>("updateChannel").filter(|c| c == "nightly").unwrap_or_else(|| "latest".into())
}

/// Semver order for `X.Y.Z[-pre]`: a release outranks its prereleases, whose dot-separated
/// parts compare numerically when both are numbers.
pub fn newer(candidate: &str, current: &str) -> bool {
    fn parts(v: &str) -> Option<([u64; 3], Option<&str>)> {
        let v = v.trim().trim_start_matches('v');
        let v = v.split('+').next()?;
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (v, None),
        };
        let mut n = core.split('.').map(|p| p.parse::<u64>().ok());
        Some(([n.next()??, n.next()??, n.next()??], pre))
    }
    let (Some((a, ap)), Some((b, bp))) = (parts(candidate), parts(current)) else { return false };
    if a != b {
        return a > b;
    }
    match (ap, bp) {
        (None, None) | (Some(_), None) => false,
        (None, Some(_)) => true,
        (Some(x), Some(y)) => {
            for (p, q) in x.split('.').zip(y.split('.')) {
                let order = match (p.parse::<u64>(), q.parse::<u64>()) {
                    (Ok(p), Ok(q)) => p.cmp(&q),
                    (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                    (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                    _ => p.cmp(q),
                };
                if order != std::cmp::Ordering::Equal {
                    return order.is_gt();
                }
            }
            x.split('.').count() > y.split('.').count()
        }
    }
}

/// The newest Rune release on `channel` from the releases API: its version and manifest URL.
/// Stable skips prereleases; nightly takes either.
pub fn pick(releases: &Value, channel: &str) -> Option<(String, String)> {
    releases
        .as_array()?
        .iter()
        .filter(|r| r["draft"] != true && (channel == "nightly" || r["prerelease"] != true))
        .filter_map(|r| {
            let version = r["tag_name"].as_str()?.strip_prefix(TAG_PREFIX)?.to_owned();
            let manifest = r["assets"].as_array()?.iter().find(|a| a["name"] == MANIFEST)?;
            Some((version, manifest["browser_download_url"].as_str()?.to_owned()))
        })
        .fold(None, |best: Option<(String, String)>, next| match best {
            Some(best) if !newer(&next.0, &best.0) => Some(best),
            _ => Some(next),
        })
}

/// This platform's archive in a release's manifest.
pub fn release_from(manifest: &Value, version: &str, platform: &str) -> Result<Release, String> {
    if manifest["version"].as_str() != Some(version) {
        return Err(format!("The {version} release's manifest describes another version."));
    }
    let file = &manifest["files"][platform];
    let (Some(name), Some(sha256), Some(size)) = (file["name"].as_str(), file["sha256"].as_str(), file["size"].as_u64()) else {
        return Err(format!("S5 Code {version} has no build for this machine yet."));
    };
    let valid_name = !name.is_empty() && !name.contains(['/', '\\']) && !name.starts_with('.');
    if !valid_name || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("The {version} release's manifest is malformed."));
    }
    Ok(Release {
        version: version.to_owned(),
        notes: manifest["notes"].as_str().unwrap_or_default().to_owned(),
        url: format!("https://github.com/{REPO}/releases/download/{TAG_PREFIX}{version}/{name}"),
        name: name.to_owned(),
        sha256: sha256.to_ascii_lowercase(),
        size,
    })
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(30)).user_agent("s5code-rune-updater").build()
}

fn get_json(url: &str) -> Result<Value, String> {
    let body = agent().get(url).set("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&body).map_err(|e| e.to_string())
}

/// Electron's `offerable`: a release on the track, never below a track switch's checked
/// release, and without a switch only a newer one on this build's own track.
pub fn offerable(candidate: &str, channel: &str, floor: Option<&str>, current: &str) -> bool {
    let track = |v: &str| if crate::settings::about::release_channel(v) == "nightly" { "nightly" } else { "latest" };
    track(candidate) == channel
        && match floor {
            Some(floor) => !newer(floor, candidate) && candidate != current,
            None => track(current) == channel && newer(candidate, current),
        }
}

fn fetch(channel: &str, floor: Option<&str>) -> Result<Option<Release>, String> {
    let platform = platform().ok_or("No builds for this platform.")?;
    let releases = get_json(&format!("https://api.github.com/repos/{REPO}/releases?per_page=50"))?;
    let Some((latest, manifest)) = pick(&releases, channel) else { return Ok(None) };
    if !offerable(&latest, channel, floor, version()) {
        return Ok(None);
    }
    release_from(&get_json(&manifest)?, &latest, platform).map(Some)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Streams `reader` to `out`, returning its SHA-256 and length.
fn copy_hashed(reader: &mut dyn Read, out: &mut dyn Write, total: u64, progress: &dyn Fn(f32)) -> std::io::Result<(String, u64)> {
    let mut digest = ring::digest::Context::new(&ring::digest::SHA256);
    let (mut buf, mut done, mut shown) = (vec![0u8; 256 * 1024], 0u64, -1i64);
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
        out.write_all(&buf[..n])?;
        done += n as u64;
        let percent = if total > 0 { (done * 100 / total).min(100) as i64 } else { 0 };
        if percent != shown {
            shown = percent;
            progress(percent as f32);
        }
    }
    out.flush()?;
    Ok((hex(digest.finish().as_ref()), done))
}

/// Checks the size and SHA-256 the manifest names, as electron-updater checks `sha512`.
pub fn verify(release: &Release, sha256: &str, size: u64) -> Result<(), String> {
    if size != release.size || !sha256.eq_ignore_ascii_case(&release.sha256) {
        return Err(format!("The downloaded {} didn't match its release's checksum, so it was discarded.", release.version));
    }
    Ok(())
}

fn staging(version: &str) -> PathBuf {
    std::env::temp_dir().join("s5code-rune-update").join(version)
}

fn download(release: &Release, progress: &dyn Fn(f32)) -> Result<PathBuf, String> {
    let dir = staging(&release.version);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(&release.name);
    let part = dir.join(format!("{}.part", release.name));
    let response = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(60)).user_agent("s5code-rune-updater").build().get(&release.url).call().map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let (sha256, size) = copy_hashed(&mut response.into_reader(), &mut file, release.size, progress).map_err(|e| e.to_string())?;
    drop(file);
    if let Err(e) = verify(release, &sha256, size) {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    std::fs::rename(&part, &path).map_err(|e| e.to_string())?;
    Ok(path)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

/// Electron's `macBundleReplaceShellScript`: once the app exits, swap the bundle (restoring the
/// old one if the copy fails) and open whichever is in place.
pub fn mac_script(pid: u32, source: &Path, dest: &Path, cleanup: &Path) -> String {
    let (source, dest, cleanup) = (quote(&source.to_string_lossy()), quote(&dest.to_string_lossy()), quote(&cleanup.to_string_lossy()));
    format!(
        "#!/bin/bash\nset -eu\nwhile kill -0 {pid} 2>/dev/null; do\n  sleep 0.2\ndone\nbackup={dest}.update-backup\nrm -rf \"$backup\"\nif ! mv {dest} \"$backup\"; then\n  exit 1\nfi\nif ditto {source} {dest}; then\n  xattr -cr {dest} || true\n  rm -rf \"$backup\" {cleanup}\n  open {dest}\nelse\n  rm -rf {dest}\n  mv \"$backup\" {dest}\n  rm -rf {cleanup}\n  open {dest}\n  exit 1\nfi\n"
    )
}

/// The Linux build is a directory of binaries: once the app exits, each new file is renamed over
/// its old one (a rename, so nothing ever runs a half-written file) and the app starts again.
pub fn linux_script(pid: u32, source: &Path, dest: &Path, files: &[String], exe: &Path, cleanup: &Path) -> String {
    let mut script = format!("#!/bin/bash\nset -u\nwhile kill -0 {pid} 2>/dev/null; do\n  sleep 0.2\ndone\n");
    for file in files {
        let (from, staged, to) = (quote(&source.join(file).to_string_lossy()), quote(&dest.join(format!(".{file}.update")).to_string_lossy()), quote(&dest.join(file).to_string_lossy()));
        script.push_str(&format!("cp -p {from} {staged} && mv -f {staged} {to}\n"));
    }
    script.push_str(&format!("rm -rf {}\nsetsid {} >/dev/null 2>&1 < /dev/null &\n", quote(&cleanup.to_string_lossy()), quote(&exe.to_string_lossy())));
    script
}

fn run(program: &str, args: &[&OsStr]) -> Result<String, String> {
    let out = std::process::Command::new(program).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("{program}: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("{program} failed: {}", err.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Electron's `resolveMacAppBundlePath`.
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(Path::to_path_buf)
}

/// What an archive holds once unpacked and checked: macOS's app bundle, or Linux's binaries.
enum Unpacked {
    App(PathBuf),
    Files(Vec<String>),
}

/// Unpacks `archive` into `unpacked` and checks it is this app: on macOS one bundle with this
/// app's id whose signature verifies, on Linux a set of files that includes `s5code-rune`.
fn unpack(archive: &Path, unpacked: &Path) -> Result<Unpacked, String> {
    let _ = std::fs::remove_dir_all(unpacked);
    std::fs::create_dir_all(unpacked).map_err(|e| e.to_string())?;
    if cfg!(target_os = "macos") {
        run("ditto", &[OsStr::new("-x"), OsStr::new("-k"), archive.as_os_str(), unpacked.as_os_str()])?;
        let apps: Vec<PathBuf> = std::fs::read_dir(unpacked).map_err(|e| e.to_string())?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "app")).collect();
        let [app] = apps.as_slice() else { return Err("The update didn't contain exactly one app.".into()) };
        let plist = app.join("Contents/Info.plist");
        let id = run("plutil", &[OsStr::new("-extract"), OsStr::new("CFBundleIdentifier"), OsStr::new("raw"), OsStr::new("-o"), OsStr::new("-"), plist.as_os_str()])?;
        if id.trim() != BUNDLE_ID {
            return Err(format!("The update is a different app ({}).", id.trim()));
        }
        run("codesign", &[OsStr::new("--verify"), OsStr::new("--deep"), OsStr::new("--strict"), app.as_os_str()])?;
        return Ok(Unpacked::App(app.clone()));
    }
    run("tar", &[OsStr::new("-xzf"), archive.as_os_str(), OsStr::new("-C"), unpacked.as_os_str()])?;
    let mut files: Vec<String> = std::fs::read_dir(unpacked).map_err(|e| e.to_string())?.flatten().filter(|e| e.path().is_file()).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    if !files.iter().any(|f| f == "s5code-rune") {
        return Err("The update has no s5code-rune.".into());
    }
    Ok(Unpacked::Files(files))
}

/// Unpacks and checks the downloaded archive, then writes the script that installs it.
fn prepare(archive: &Path) -> Result<PathBuf, String> {
    let dir = archive.parent().ok_or("No staging folder.")?.to_path_buf();
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let pid = std::process::id();
    let script = match unpack(archive, &dir.join("unpacked"))? {
        Unpacked::App(app) => {
            let bundle = bundle_of(&exe).ok_or("This copy of S5 Code isn't an app bundle, so it can't update itself.")?;
            mac_script(pid, &app, &bundle, &dir)
        }
        Unpacked::Files(files) => {
            let dest = exe.parent().ok_or("No install folder.")?.to_path_buf();
            let name = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if !files.contains(&name) {
                return Err(format!("The update has no {name}."));
            }
            let probe = dest.join(".s5code-update-probe");
            std::fs::write(&probe, b"").map_err(|_| format!("S5 Code can't write to {}, so it can't update itself there.", dest.display()))?;
            let _ = std::fs::remove_file(&probe);
            linux_script(pid, &dir.join("unpacked"), &dest, &files, &exe, &dir)
        }
    };
    let path = dir.join("install.sh");
    std::fs::write(&path, script).map_err(|e| e.to_string())?;
    Ok(path)
}

/// `s5code-rune --check-update-archive <manifest> <archive>`: the release workflow's check that
/// this build reads the manifest it publishes, and that the archive passes the checksum and
/// unpack checks an install runs. Exits non-zero with the reason on failure.
pub fn check_archive(manifest: &Path, archive: &Path) -> i32 {
    let result = (|| -> Result<String, String> {
        let manifest: Value = serde_json::from_str(&std::fs::read_to_string(manifest).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let version = manifest["version"].as_str().ok_or("The manifest has no version.")?;
        if version != self::version() {
            return Err(format!("The manifest is for {version}, this build is {}.", self::version()));
        }
        let release = release_from(&manifest, version, platform().ok_or("No builds for this platform.")?)?;
        let mut file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
        let (sha256, size) = copy_hashed(&mut file, &mut std::io::sink(), release.size, &|_| {}).map_err(|e| e.to_string())?;
        verify(&release, &sha256, size)?;
        let unpacked = std::env::temp_dir().join(format!("s5code-rune-check-{}", std::process::id()));
        let what = match unpack(archive, &unpacked) {
            Ok(Unpacked::App(app)) => format!("app {}", app.file_name().unwrap_or_default().to_string_lossy()),
            Ok(Unpacked::Files(files)) => format!("files {}", files.join(", ")),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&unpacked);
                return Err(e);
            }
        };
        let _ = std::fs::remove_dir_all(&unpacked);
        Ok(format!("{} {} sha256={sha256} size={size} {what}", release.version, release.name))
    })();
    match result {
        Ok(summary) => {
            println!("check-update-archive: ok {summary}");
            0
        }
        Err(e) => {
            eprintln!("check-update-archive: {e}");
            1
        }
    }
}

fn spawn(s: &App, work: impl FnOnce(&dyn Fn(Event)) + Send + 'static) {
    let events = s.ui.desktop.updates.events.clone();
    let waker = s.ui.panel.waker.clone();
    std::thread::spawn(move || {
        let send = |event: Event| {
            events.lock().unwrap_or_else(|e| e.into_inner()).push(event);
            if let Some(wake) = &waker {
                wake();
            }
        };
        work(&send);
    });
}

/// "Check for Updates": from the menus and Settings › About, which show the outcome.
pub fn check(s: &mut App, announce: bool) {
    if let Some(reason) = disabled(s) {
        if announce {
            super::show_alert(s, "Updates unavailable", &reason);
        }
        return;
    }
    let ui = &mut s.ui.desktop.updates;
    ui.announce |= announce;
    if matches!(ui.status, Status::Checking | Status::Downloading(_) | Status::Installing) {
        return;
    }
    ui.status = Status::Checking;
    let (channel, floor) = (channel(), ui.floor.clone());
    spawn(s, move |send| send(Event::Checked(fetch(&channel, floor.as_deref()))));
}

/// After the track switch saved `updateChannel`: sets the floor, drops an offer the new track
/// doesn't make, and checks it.
pub fn set_channel(s: &mut App, minimum_version: Option<String>) {
    let ui = &mut s.ui.desktop.updates;
    ui.floor = minimum_version;
    let retained = ui.release.as_ref().is_some_and(|r| offerable(&r.version, &channel(), ui.floor.as_deref(), version()));
    if ui.release.is_some() && !retained && !matches!(ui.status, Status::Downloading(_) | Status::Installing) {
        ui.release = None;
        ui.staged = None;
        ui.status = Status::Idle;
    }
    check(s, false);
}

pub fn download_update(s: &mut App) {
    let Some(release) = s.ui.desktop.updates.release.clone() else { return };
    if matches!(s.ui.desktop.updates.status, Status::Downloading(_)) {
        return;
    }
    s.ui.desktop.updates.status = Status::Downloading(0.0);
    spawn(s, move |send| {
        let result = download(&release, &|p| send(Event::Progress(p)));
        send(Event::Downloaded(result));
    });
}

pub(crate) fn install(s: &mut App) {
    let Some(archive) = s.ui.desktop.updates.staged.clone() else { return };
    if let Some(release) = &s.ui.desktop.updates.release {
        crate::shell::sidebar::everything::note_install(&release.version);
    }
    s.ui.desktop.updates.confirm = false;
    s.ui.desktop.updates.status = Status::Installing;
    spawn(s, move |send| send(Event::Prepared(prepare(&archive))));
}

/// The pill's and About's button: check, download, or confirm the install.
pub fn press(s: &mut App) {
    match action(&s.ui.desktop.updates) {
        Action::Install => s.ui.desktop.updates.confirm = true,
        Action::Download => download_update(s),
        Action::None => check(s, false),
    }
}

pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let now = crate::data::now_ms();
    let mut changed = false;
    let live = !rune::is_headless() && disabled(s).is_none();
    match s.ui.desktop.updates.next_check {
        None if live => s.ui.desktop.updates.next_check = Some(now + STARTUP_DELAY_MS),
        Some(at) if live && now >= at => {
            s.ui.desktop.updates.next_check = Some(now + POLL_MS);
            // A background check never interrupts a download or an update waiting to install.
            if matches!(s.ui.desktop.updates.status, Status::Idle | Status::UpToDate | Status::Error(Context::Check, _)) {
                check(s, false);
                changed = true;
            }
        }
        _ => {}
    }
    let events = std::mem::take(&mut *s.ui.desktop.updates.events.lock().unwrap_or_else(|e| e.into_inner()));
    for event in events {
        changed = true;
        let ui = &mut s.ui.desktop.updates;
        match event {
            Event::Checked(result) => {
                let announce = std::mem::take(&mut ui.announce);
                match result {
                    Ok(Some(release)) => {
                        // A newer release replaces one downloaded before it.
                        if ui.release.as_ref().is_none_or(|r| r.version != release.version) {
                            ui.staged = None;
                        }
                        ui.release = Some(release);
                        if ui.staged.is_none() {
                            ui.status = Status::Available;
                        } else {
                            ui.status = Status::Downloaded;
                        }
                    }
                    Ok(None) => {
                        // The track no longer offers what was found or downloaded before.
                        ui.release = None;
                        ui.staged = None;
                        ui.status = Status::UpToDate;
                        if announce {
                            super::show_alert(s, "You're up to date!", &format!("S5 Code {} is currently the newest version available.", version()));
                        }
                    }
                    Err(message) => {
                        ui.status = Status::Error(Context::Check, message.clone());
                        if announce {
                            super::show_alert(s, "Update check failed", &format!("Could not check for updates. {message}"));
                        }
                    }
                }
            }
            Event::Progress(percent) => ui.status = Status::Downloading(percent),
            Event::Downloaded(Ok(path)) => {
                ui.staged = Some(path);
                ui.status = Status::Downloaded;
            }
            Event::Downloaded(Err(message)) => ui.status = Status::Error(Context::Download, message),
            Event::Prepared(Ok(script)) => {
                let started = std::process::Command::new("/bin/bash")
                    .arg(&script)
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn();
                match started {
                    Ok(_) => tx.quit(),
                    Err(e) => ui.status = Status::Error(Context::Install, e.to_string()),
                }
            }
            Event::Prepared(Err(message)) => ui.status = Status::Error(Context::Install, message),
        }
    }
    changed
}

/// Web `getDesktopUpdateButtonTooltip`, and the idle check's.
pub fn summary(s: &App) -> String {
    let ui = &s.ui.desktop.updates;
    let available = ui.release.as_ref().map_or("available", |r| r.version.as_str());
    match &ui.status {
        Status::Available => format!("Update {available} ready to download"),
        Status::Downloading(p) => format!("Downloading update ({}%)", *p as u32),
        Status::Downloaded => format!("Update {available} downloaded. Click to restart and install."),
        Status::Installing => format!("Installing {available}…"),
        Status::Checking => "Checking for updates…".to_owned(),
        Status::Error(Context::Download, _) if ui.release.is_some() => format!("Download failed for {available}. Click to retry."),
        Status::Error(Context::Install, _) if ui.staged.is_some() => format!("Install failed for {available}. Click to retry."),
        Status::Error(_, message) => message.clone(),
        Status::UpToDate => "Up to date".to_owned(),
        Status::Idle => disabled(s).unwrap_or_else(|| "Check for updates".to_owned()),
    }
}

/// Web `SidebarUpdatePill`'s button. While providers wait on an update and this app has nothing
/// to do, the sidebar's "update everything" button shows instead (`None`).
pub fn pill(s: &App, t: &T, providers_waiting: bool) -> Option<El> {
    let ui = &s.ui.desktop.updates;
    let action = action(ui);
    let busy = matches!(ui.status, Status::Downloading(_) | Status::Installing);
    if action == Action::None && !busy && providers_waiting {
        return None;
    }
    let disabled = disabled(s).is_some() || matches!(ui.status, Status::Checking | Status::Installing);
    let shown = action != Action::None || busy;
    let glyph = match (&ui.status, action) {
        (_, Action::Install) => &i::ROTATE_CW,
        (Status::Downloading(_), _) | (_, Action::Download) => &i::DOWNLOAD,
        _ => &i::REFRESH_CW,
    };
    let color = if shown { t.sb.sidebar_foreground } else { t.sb.sidebar_icon_color };
    let mut glyph_el = el().size(16.0, 16.0).child(icon(glyph, 16.0).color(color));
    if action == Action::Download {
        glyph_el = glyph_el.child(el().pin(None, Some(-2.0), Some(-2.0), None).size(6.0, 6.0).radius(radius::FULL).fill(color));
    } else if action == Action::Install {
        glyph_el = glyph_el.child(
            el().pin(None, None, Some(-4.0), Some(-4.0))
                .size(10.0, 10.0)
                .radius(radius::FULL)
                .fill(t.c.foreground)
                .center()
                .child(icon(&i::CHECK, 8.0).color(t.c.background)),
        );
    }
    let mut button = el()
        .key("sb-app-update")
        .size(32.0, 32.0)
        .radius(radius::FULL)
        .center()
        .role("button")
        .label(summary(s))
        .help(summary(s))
        .child(glyph_el);
    if shown {
        button = button.fill(t.sb.sidebar_control_surface);
    }
    if disabled {
        if !shown {
            button = button.opacity(0.6);
        }
    } else if !busy {
        button = button.hover(t.sb.sidebar_row_hover).press(t.sb.sidebar_row_active).cursor(Cursor::Pointer).on_press(press);
    }
    if let Status::Downloading(p) = ui.status {
        // The ring web draws around the icon, as a bar under it: no continuously repainting arc.
        button = button.child(el().pin(Some(6.0), None, None, Some(4.0)).size(20.0 * (p / 100.0).clamp(0.0, 1.0), 2.0).radius(1.0).fill(color));
    }
    Some(button)
}

/// Settings › About's version row: what the updater is doing and its next step.
pub fn about(s: &App) -> (String, Option<&'static str>) {
    let ui = &s.ui.desktop.updates;
    if let Some(reason) = disabled(s) {
        return (reason, None);
    }
    let label = match action(ui) {
        Action::Install => Some("Install"),
        Action::Download => Some("Download"),
        Action::None if matches!(ui.status, Status::Downloading(_) | Status::Installing | Status::Checking) => None,
        Action::None if matches!(ui.status, Status::UpToDate) => Some("Up to Date"),
        Action::None => Some("Check for Updates"),
    };
    let text = match &ui.status {
        Status::Idle => "Checks for updates automatically.".to_owned(),
        _ => summary(s),
    };
    (text, label)
}

/// Settings › About's inert button while the updater works: web's `statusLabel`.
pub fn about_busy_label(s: &App) -> &'static str {
    match s.ui.desktop.updates.status {
        Status::Checking => "Checking…",
        Status::Downloading(_) => "Downloading…",
        _ => "Check for Updates",
    }
}

/// Web's install confirmation (`getDesktopUpdateInstallConfirmationMessage`), with the
/// release notes the sidebar popover shows.
pub fn confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.desktop.updates;
    if !ui.confirm {
        return;
    }
    let version = ui.release.as_ref().map(|r| r.version.clone()).unwrap_or_default();
    let notes = ui.release.as_ref().map(|r| r.notes.trim().to_owned()).filter(|n| !n.is_empty());
    let close = |s: &mut App| s.ui.desktop.updates.confirm = false;
    let mut dialog = kit::Dialog::new("app-update-confirm", format!("Install update {version} and restart S5 Code?"))
        .open(true)
        .alert()
        .description("Any running tasks will be interrupted. Make sure you're ready before continuing.")
        .on_close(close);
    if let Some(notes) = notes {
        let notes: String = notes.chars().take(1200).collect();
        dialog = dialog.child(text(notes).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().max_h(240.0));
    }
    dialog
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(kit::Button::new("Restart and install").on_press(install).build(t)),
        )
        .build(t, layers);
}

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("app-update-available", |s| {
        showcase(s);
        fixture(s, Status::Available, false);
    }),
    ("app-update-downloading", |s| {
        showcase(s);
        fixture(s, Status::Downloading(42.0), false);
    }),
    ("app-update-confirm", |s| {
        showcase(s);
        fixture(s, Status::Downloaded, true);
    }),
    ("settings-about-app-update", |s| {
        crate::scene_clock(s);
        s.route = crate::Route::Settings("about".into());
        fixture(s, Status::Available, false);
    }),
];

fn showcase(s: &mut App) {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| crate::data::s(t, "id").to_string()) {
        s.navigate(crate::Route::Thread(id));
    }
}

/// Scene states for the pill, About row and dialog.
fn fixture(s: &mut App, status: Status, staged: bool) {
    let ui = &mut s.ui.desktop.updates;
    ui.fixture = true;
    ui.release = Some(Release {
        version: "0.1.3".into(),
        notes: "Faster device streaming, a Speech menu, and fixes for notifications.".into(),
        url: String::new(),
        name: "S5-Code-Rune-0.1.3-macos-arm64.zip".into(),
        sha256: "0".repeat(64),
        size: 1,
    });
    ui.staged = staged.then(|| PathBuf::from("/tmp/s5code-rune-update/0.1.3/S5-Code-Rune-0.1.3-macos-arm64.zip"));
    ui.confirm = status == Status::Downloaded && staged;
    ui.status = status;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn about_s_button_reads_as_web_s() {
        let mut s = App::new();
        let label = |s: &mut App, status: Status, staged: bool| {
            fixture(s, status, staged);
            about(s).1.unwrap_or_else(|| about_busy_label(s))
        };
        assert_eq!(label(&mut s, Status::Available, false), "Download");
        assert_eq!(label(&mut s, Status::Downloaded, true), "Install");
        assert_eq!(label(&mut s, Status::Downloading(42.0), false), "Downloading…");
        assert_eq!(label(&mut s, Status::Checking, false), "Checking…");
        assert_eq!(label(&mut s, Status::UpToDate, false), "Up to Date");
        assert_eq!(label(&mut s, Status::Idle, false), "Check for Updates");
    }

    #[test]
    fn versions_order_as_semver() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("1.0.0", "0.9.99"));
        assert!(!newer("0.1.3", "0.1.3"));
        assert!(newer("0.1.3", "0.1.3-nightly.20261122.4"), "a release outranks its prereleases");
        assert!(!newer("0.1.3-nightly.20261122.4", "0.1.3"));
        assert!(newer("0.1.3-nightly.20261122.10", "0.1.3-nightly.20261122.9"), "numeric parts compare as numbers");
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(!newer("garbage", "0.1.0") && !newer("0.1", "0.0.1"));
    }

    #[test]
    fn a_track_switch_offers_nothing_below_its_checked_release() {
        let nightly = "1.2.4-nightly.20261027.1";
        assert!(!offerable("1.2.2", "latest", Some("1.2.3"), nightly), "below the switch's checked release");
        assert!(offerable("1.2.3", "latest", Some("1.2.3"), nightly), "an older Stable, but the one the switch checked");
        assert!(!offerable("1.2.3", "latest", None, nightly), "a saved Stable track after a restart crosses only through a switch");
        assert!(!offerable("1.2.5-nightly.20261101.1", "latest", Some("1.2.3"), nightly), "another track's release");
        assert!(offerable("1.2.5-nightly.20261101.1", "nightly", None, nightly));
        assert!(!offerable(nightly, "nightly", None, nightly), "the running build");
        assert!(!offerable("1.2.3", "latest", Some("1.2.3"), "1.2.3"));
    }

    #[test]
    fn the_feed_is_the_newest_rune_release_on_the_track() {
        let asset = |tag: &str| json!([{"name": MANIFEST, "browser_download_url": format!("https://example.test/{tag}/rune-update.json")}]);
        let releases = json!([
            {"tag_name": "v0.2.71", "draft": false, "prerelease": false, "assets": [{"name": "latest-mac.yml"}]},
            {"tag_name": "rune-v0.1.4-nightly.20261122.1", "draft": false, "prerelease": true, "assets": asset("n")},
            {"tag_name": "rune-v0.1.9", "draft": true, "prerelease": false, "assets": asset("draft")},
            {"tag_name": "rune-v0.1.2", "draft": false, "prerelease": false, "assets": asset("old")},
            {"tag_name": "rune-v0.1.3", "draft": false, "prerelease": false, "assets": asset("new")},
            {"tag_name": "rune-v0.1.5", "draft": false, "prerelease": false, "assets": []},
        ]);
        assert_eq!(pick(&releases, "latest"), Some(("0.1.3".into(), "https://example.test/new/rune-update.json".into())), "drafts, prereleases and releases without a manifest are skipped");
        assert_eq!(pick(&releases, "nightly").map(|p| p.0), Some("0.1.4-nightly.20261122.1".into()));
        assert_eq!(pick(&json!([]), "latest"), None);
    }

    #[test]
    fn a_manifest_names_this_platforms_archive_and_its_checksum() {
        let sha = "ab".repeat(32);
        let manifest = json!({"version": "0.1.3", "notes": "Notes", "files": {"macos-arm64": {"name": "S5-Code-Rune-0.1.3-macos-arm64.zip", "sha256": sha, "size": 42}}});
        let release = release_from(&manifest, "0.1.3", "macos-arm64").unwrap();
        assert_eq!(release.url, "https://github.com/SparshKaushik/s5code/releases/download/rune-v0.1.3/S5-Code-Rune-0.1.3-macos-arm64.zip");
        assert_eq!((release.size, release.notes.as_str()), (42, "Notes"));
        assert!(release_from(&manifest, "0.1.3", "linux-x64").unwrap_err().contains("no build for this machine"));
        assert!(release_from(&manifest, "0.1.4", "macos-arm64").is_err(), "the manifest must be the tag's");
        let bad = json!({"version": "0.1.3", "files": {"linux-x64": {"name": "../../evil", "sha256": sha, "size": 1}}});
        assert!(release_from(&bad, "0.1.3", "linux-x64").unwrap_err().contains("malformed"));
    }

    #[test]
    fn a_download_is_kept_only_when_its_checksum_and_size_match() {
        let body = b"S5 Code".repeat(100_000);
        let mut out = Vec::new();
        let seen = std::cell::RefCell::new(Vec::new());
        let (sha, size) = copy_hashed(&mut &body[..], &mut out, body.len() as u64, &|p| seen.borrow_mut().push(p)).unwrap();
        assert_eq!((out.len(), size), (body.len(), body.len() as u64));
        assert_eq!(sha, hex(ring::digest::digest(&ring::digest::SHA256, &body).as_ref()));
        assert_eq!(seen.borrow().last(), Some(&100.0));
        let release = Release { version: "0.1.3".into(), notes: String::new(), url: String::new(), name: "a.zip".into(), sha256: sha.to_uppercase(), size };
        assert!(verify(&release, &sha, size).is_ok());
        assert!(verify(&release, &sha, size - 1).is_err());
        assert!(verify(&release, &"0".repeat(64), size).is_err());
        assert_eq!(hex(ring::digest::digest(&ring::digest::SHA256, b"").as_ref()), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    }

    /// The install scripts are POSIX shell, for macOS and Linux.
    #[cfg(unix)]
    #[test]
    fn install_scripts_wait_for_the_app_and_quote_paths() {
        let mac = mac_script(42, Path::new("/tmp/u/S5 Code (Rune).app"), Path::new("/Applications/S5 Code (Rune).app"), Path::new("/tmp/u"));
        assert!(mac.contains("while kill -0 42"));
        assert!(mac.contains("ditto '/tmp/u/S5 Code (Rune).app' '/Applications/S5 Code (Rune).app'"), "{mac}");
        assert!(mac.contains("mv \"$backup\" '/Applications/S5 Code (Rune).app'"), "a failed copy restores the old app");
        let linux = linux_script(7, Path::new("/tmp/u/unpacked"), Path::new("/opt/it's"), &["s5code-rune".into(), "s5code-server".into()], Path::new("/opt/it's/s5code-rune"), Path::new("/tmp/u"));
        assert!(linux.contains("cp -p '/tmp/u/unpacked/s5code-server' '/opt/it'\\''s/.s5code-server.update' && mv -f '/opt/it'\\''s/.s5code-server.update' '/opt/it'\\''s/s5code-server'"), "{linux}");
        assert!(linux.trim_end().ends_with("setsid '/opt/it'\\''s/s5code-rune' >/dev/null 2>&1 < /dev/null &"));
        assert_eq!(bundle_of(Path::new("/Applications/S5 Code (Rune).app/Contents/MacOS/s5code-rune")), Some(PathBuf::from("/Applications/S5 Code (Rune).app")));
        assert_eq!(bundle_of(Path::new("/opt/s5/s5code-rune")), None);
    }

    #[test]
    fn the_button_downloads_then_installs() {
        let mut ui = Ui::default();
        assert_eq!(action(&ui), Action::None);
        ui.release = Some(Release { version: "0.1.3".into(), notes: String::new(), url: String::new(), name: "a".into(), sha256: String::new(), size: 0 });
        ui.status = Status::Available;
        assert_eq!(action(&ui), Action::Download);
        ui.status = Status::Error(Context::Download, "x".into());
        assert_eq!(action(&ui), Action::Download, "a failed download retries");
        ui.staged = Some(PathBuf::from("/tmp/a"));
        ui.status = Status::Downloaded;
        assert_eq!(action(&ui), Action::Install);
        ui.status = Status::Error(Context::Install, "x".into());
        assert_eq!(action(&ui), Action::Install, "a failed install retries");
    }
}
