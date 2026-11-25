//! Video where Rune's player can't play it (everywhere but macOS): an MP4 or QuickTime file's
//! H.264 track, demuxed here and decoded by the device panel's decoder (OpenH264 on Linux), its
//! frames drawn as pixels on their presentation times. Silent until an audio stack lands.

use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rune::Pixels;

use super::mp4::{self, Track};
use crate::panel::device::h264;

/// Larger than a picture's cap: a short screen recording runs to tens of megabytes.
const MAX_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Loading,
    Ready,
    Failed(String),
}

#[derive(Default)]
struct State {
    status: Status,
    playing: bool,
    size: Option<(u32, u32)>,
    /// Every handle is gone: the worker stops.
    dropped: bool,
}

struct Shared {
    pixels: Pixels,
    state: Mutex<State>,
    changed: Condvar,
    waker: Option<rune::Waker>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn wake(&self) {
        if let Some(wake) = &self.waker {
            wake();
        }
    }

    fn fail(&self, why: String) {
        self.lock().status = Status::Failed(why);
        self.changed.notify_all();
        self.wake();
    }

    fn show(&self, frame: h264::Frame) {
        let size = (frame.width, frame.height);
        self.pixels.set(frame.width, frame.height, frame.rgba);
        let mut state = self.lock();
        state.size = Some(size);
        if state.status == Status::Loading {
            state.status = Status::Ready;
        }
        drop(state);
        self.changed.notify_all();
        self.wake();
    }
}

/// Stops the worker once the last player handle drops.
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        self.0.lock().dropped = true;
        self.0.changed.notify_all();
    }
}

/// A cheap handle, like Rune's `Player`: clones share one worker.
#[derive(Clone)]
pub struct SoftPlayer {
    shared: Arc<Shared>,
    _handle: Arc<Handle>,
}

struct Clip {
    file: Vec<u8>,
    track: Track,
    order: Vec<i64>,
}

impl SoftPlayer {
    fn new(waker: Option<rune::Waker>) -> Self {
        let shared = Arc::new(Shared { pixels: Pixels::new(), state: Mutex::default(), changed: Condvar::new(), waker });
        SoftPlayer { _handle: Arc::new(Handle(shared.clone())), shared }
    }

    /// Reads the clip at `url` off-thread, shows its first frame, and waits for `play`.
    pub fn url(url: String, waker: Option<rune::Waker>) -> Self {
        let player = Self::new(waker);
        let shared = player.shared.clone();
        std::thread::spawn(move || match read(&url).and_then(open).and_then(|clip| poster(&shared, clip)) {
            Ok(clip) => serve(&shared, clip),
            Err(why) => shared.fail(why),
        });
        player
    }

    /// A clip already in memory, its first frame shown before this returns (scenes).
    pub fn bytes(file: Vec<u8>, waker: Option<rune::Waker>) -> Self {
        let player = Self::new(waker);
        match open(file).and_then(|clip| poster(&player.shared, clip)) {
            Ok(clip) => {
                let shared = player.shared.clone();
                std::thread::spawn(move || serve(&shared, clip));
            }
            Err(why) => player.shared.fail(why),
        }
        player
    }

    pub fn pixels(&self) -> &Pixels {
        &self.shared.pixels
    }

    pub fn status(&self) -> Status {
        self.shared.lock().status.clone()
    }

    pub fn natural_size(&self) -> Option<(u32, u32)> {
        self.shared.lock().size
    }

    pub fn is_playing(&self) -> bool {
        self.shared.lock().playing
    }

    pub fn play(&self) {
        self.set_playing(true);
    }

    pub fn pause(&self) {
        self.set_playing(false);
    }

    fn set_playing(&self, playing: bool) {
        self.shared.lock().playing = playing;
        self.shared.changed.notify_all();
        self.shared.wake();
    }
}

fn read(url: &str) -> Result<Vec<u8>, String> {
    if url.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("data:")) {
        return super::fetch(url);
    }
    use std::io::Read;
    let mut data = Vec::new();
    super::get(url)?.into_reader().take(MAX_BYTES).read_to_end(&mut data).map_err(|e| e.to_string())?;
    Ok(data)
}

fn open(file: Vec<u8>) -> Result<Clip, String> {
    let track = mp4::parse(&file)?;
    let order = track.display_order();
    Ok(Clip { file, track, order })
}

/// Shows the clip's first frame.
fn poster(shared: &Shared, clip: Clip) -> Result<Clip, String> {
    pass(shared, &clip, true).map(|_| clip)
}

/// Plays each time `play` is asked, from the start once it has ended.
fn serve(shared: &Shared, clip: Clip) {
    loop {
        let state = shared.changed.wait_while(shared.lock(), |s| !s.playing && !s.dropped).unwrap_or_else(|e| e.into_inner());
        if state.dropped {
            return;
        }
        drop(state);
        match pass(shared, &clip, false) {
            Ok(true) => return,
            Ok(false) => {}
            Err(why) => return shared.fail(why),
        }
        shared.lock().playing = false;
        shared.changed.notify_all();
        shared.wake();
    }
}

/// One pass through the clip: just its first frame for the poster, else every frame on time.
/// True when the player was dropped.
fn pass(shared: &Shared, clip: &Clip, poster: bool) -> Result<bool, String> {
    let mut decoder = h264::Decoder::new(&clip.track.params)?;
    let rate = f64::from(clip.track.timescale.max(1));
    let mut clock: Option<(Instant, i64)> = None;
    let mut shown = 0;
    for sample in &clip.track.samples {
        let nals = h264::length_nals(&clip.file[sample.offset..sample.offset + sample.size], clip.track.length_size);
        // A damaged frame is skipped, as a player would.
        let Ok(Some(frame)) = decoder.decode(&nals) else { continue };
        let pts = clip.order.get(shown).copied().unwrap_or(sample.pts);
        shown += 1;
        if poster {
            shared.show(frame);
            return Ok(false);
        }
        let mut state = shared.lock();
        loop {
            if state.dropped {
                return Ok(true);
            }
            if !state.playing {
                // Paused: the clock restarts at this frame on resume.
                clock = None;
                state = shared.changed.wait(state).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            let (start, base) = *clock.get_or_insert((Instant::now(), pts));
            let due = start + Duration::from_secs_f64((pts - base).max(0) as f64 / rate);
            let now = Instant::now();
            if now >= due {
                break;
            }
            state = shared.changed.wait_timeout(state, due - now).unwrap_or_else(|e| e.into_inner()).0;
        }
        drop(state);
        shared.show(frame);
    }
    if shown == 0 {
        return Err("The video has no frame this decoder can read.".into());
    }
    Ok(false)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn a_clip_shows_its_first_frame_then_plays_to_the_end() {
        let player = SoftPlayer::bytes(super::super::scene_clip(), None);
        assert_eq!((player.status(), player.natural_size(), player.pixels().size()), (Status::Ready, Some((320, 176)), (320, 176)), "FAILED: the poster shows before bytes returns");
        assert!(!player.is_playing(), "FAILED: the poster waits for play");
        player.play();
        // The ended pass sets playing back to false and leaves the last frame up.
        drop(player.shared.changed.wait_while(player.shared.lock(), |s| s.playing).unwrap());
        assert_eq!(player.status(), Status::Ready);
    }

    #[test]
    fn what_is_not_h264_mp4_fails_with_the_reason() {
        let player = SoftPlayer::bytes(b"\x1a\x45\xdf\xa3webm".to_vec(), None);
        assert_eq!(player.status(), Status::Failed(mp4::UNSUPPORTED.into()));
    }
}
