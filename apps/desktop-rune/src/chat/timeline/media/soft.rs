//! Video where Rune's player can't play it (everywhere but macOS): an MP4 or QuickTime file's
//! H.264 track, demuxed here and decoded by the device panel's decoder (OpenH264 on Linux), its
//! frames drawn as pixels on their presentation times. Its sound is Rune's audio player reading
//! the same download from a temporary file (symphonia's MP4 and AAC on Linux), and the sound is
//! the clock: each frame shows when the sound reaches it, so long clips don't drift.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rune::{Pixels, PlayerState};

use super::mp4::{self, Track};
use crate::panel::device::h264;

/// Larger than a picture's cap: a short screen recording runs to tens of megabytes.
const MAX_BYTES: u64 = 512 * 1024 * 1024;
/// How long frames wait for the sound to start before keeping their own time.
const SOUND_WAIT: Duration = Duration::from_secs(3);
/// A frame this far behind the sound is dropped to catch up.
const LATE: f64 = 0.1;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Status {
    #[default]
    Loading,
    Ready,
    Failed(String),
}

/// The sound's clock as the main thread last read it, which the frames follow.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Sound {
    /// No sound, or it failed or ended: frames keep their own time.
    Off,
    /// Not started yet, or paused: frames wait for it a while.
    Waiting,
    /// Seconds into the clip, as read at that instant.
    At(f64, Instant),
}

struct State {
    status: Status,
    playing: bool,
    size: Option<(u32, u32)>,
    /// The next play starts from the first frame: nothing has played yet, or the clip ended.
    at_start: bool,
    /// Every handle is gone: the worker stops.
    dropped: bool,
    sound: Sound,
    /// The download, written out for the audio player to read; removed with the last handle.
    sound_file: Option<PathBuf>,
    /// Seconds into the clip of the frame on screen.
    position: f64,
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

    fn show(&self, frame: h264::Frame, at: f64) {
        let size = (frame.width, frame.height);
        self.pixels.set(frame.width, frame.height, frame.rgba);
        let mut state = self.lock();
        (state.size, state.position) = (Some(size), at);
        if state.status == Status::Loading {
            state.status = Status::Ready;
        }
        drop(state);
        self.changed.notify_all();
        self.wake();
    }
}

impl Default for State {
    fn default() -> Self {
        State { status: Status::Loading, playing: false, size: None, at_start: true, dropped: false, sound: Sound::Off, sound_file: None, position: 0.0 }
    }
}

/// Stops the worker once the last player handle drops.
struct Handle(Arc<Shared>);

impl Drop for Handle {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.dropped = true;
        if let Some(path) = state.sound_file.take() {
            let _ = std::fs::remove_file(path);
        }
        drop(state);
        self.0.changed.notify_all();
    }
}

/// A cheap handle, like Rune's `Player`: clones share one worker.
#[derive(Clone)]
pub struct SoftPlayer {
    shared: Arc<Shared>,
    /// The clip's sound, made on the main thread once its file is written. Dropped before the
    /// handle removes that file.
    audio: Rc<RefCell<Option<rune::Player>>>,
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
        SoftPlayer { _handle: Arc::new(Handle(shared.clone())), shared, audio: Rc::default() }
    }

    /// Reads the clip at `url` off-thread, shows its first frame, and waits for `play`.
    pub fn url(url: String, waker: Option<rune::Waker>) -> Self {
        let player = Self::new(waker);
        player.shared.lock().sound = Sound::Waiting;
        let shared = player.shared.clone();
        std::thread::spawn(move || match read(&url).and_then(open).map(|clip| keep_sound(&shared, clip)).and_then(|clip| poster(&shared, clip)) {
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
        if let Some(audio) = &*self.audio.borrow() {
            if self.shared.lock().at_start {
                audio.seek(0.0);
            }
            audio.play();
        }
        self.set_playing(true);
    }

    pub fn pause(&self) {
        if let Some(audio) = &*self.audio.borrow() {
            audio.pause();
        }
        self.set_playing(false);
    }

    fn set_playing(&self, playing: bool) {
        let mut state = self.shared.lock();
        state.playing = playing;
        // The clock read before a pause or rewind no longer holds; frames wait for a fresh one.
        if self.audio.borrow().is_some() {
            state.sound = Sound::Waiting;
        }
        drop(state);
        self.shared.changed.notify_all();
        self.shared.wake();
    }

    /// Reads the sound's clock for the frames, from the main thread where Rune's audio player
    /// tells its time: once the clip's file is written, makes the player, and pauses it when
    /// the frames have ended. Called from `media::poll`, which each frame shown wakes.
    pub fn sync(&self) {
        let mut audio = self.audio.borrow_mut();
        let mut state = self.shared.lock();
        if audio.is_none() {
            let Some(path) = state.sound_file.clone() else { return };
            let player = rune::Player::audio(path);
            if let Some(wake) = &self.shared.waker {
                player.set_waker(wake.clone());
            }
            player.preload();
            if state.playing {
                player.seek(state.position);
                player.play();
            }
            *audio = Some(player);
        }
        let Some(player) = audio.as_ref() else { return };
        if !state.playing && player.is_playing() {
            player.pause();
        }
        let sound = match player.state() {
            PlayerState::Loading | PlayerState::Ready => Sound::Waiting,
            PlayerState::Playing => match (player.time(), state.sound) {
                // The time moves in steps: frames between them count on from the last reading.
                (time, Sound::At(was, when)) if time == was => Sound::At(was, when),
                (time, _) => Sound::At(time, Instant::now()),
            },
            // Ended or failed.
            _ => Sound::Off,
        };
        if sound != state.sound {
            state.sound = sound;
            drop(state);
            self.shared.changed.notify_all();
        }
    }
}

/// Writes a clip with sound out for the audio player, which reads files and URLs, not bytes.
fn keep_sound(shared: &Shared, clip: Clip) -> Clip {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("s5code-clip-{}-{}.mp4", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    let written = mp4::has_sound(&clip.file) && std::fs::write(&path, &clip.file).is_ok();
    let mut state = shared.lock();
    if written && !state.dropped {
        state.sound_file = Some(path);
    } else {
        if written {
            let _ = std::fs::remove_file(&path);
        }
        state.sound = Sound::Off;
    }
    drop(state);
    shared.changed.notify_all();
    shared.wake();
    clip
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
        let mut state = state;
        state.at_start = false;
        drop(state);
        match pass(shared, &clip, false) {
            Ok(true) => return,
            Ok(false) => {}
            Err(why) => return shared.fail(why),
        }
        let mut state = shared.lock();
        (state.playing, state.at_start) = (false, true);
        drop(state);
        shared.changed.notify_all();
        shared.wake();
    }
}

/// One pass through the clip: just its first frame for the poster, else every frame on time.
/// True when the player was dropped.
fn pass(shared: &Shared, clip: &Clip, poster: bool) -> Result<bool, String> {
    let mut decoder = h264::Decoder::reordering(&clip.track.params)?;
    let rate = f64::from(clip.track.timescale.max(1));
    let mut pace = Pace::default();
    let (mut next, mut shown) = (0, 0);
    let mut tail: Option<std::vec::IntoIter<h264::Frame>> = None;
    loop {
        let frame = if let Some(sample) = clip.track.samples.get(next) {
            next += 1;
            let nals = h264::length_nals(&clip.file[sample.offset..sample.offset + sample.size], clip.track.length_size);
            // A damaged frame is skipped, as a player would.
            let Ok(Some(frame)) = decoder.decode(&nals) else { continue };
            frame
        } else {
            // The pictures held back for reordering come out once the stream ends.
            let Some(frame) = tail.get_or_insert_with(|| decoder.finish().unwrap_or_default().into_iter()).next() else { break };
            frame
        };
        // Frames come back in display order, so the nth one shows at the nth earliest time.
        let at = clip.order.get(shown).map_or(0.0, |pts| *pts as f64 / rate);
        shown += 1;
        if poster {
            shared.show(frame, at);
            return Ok(false);
        }
        match pace.wait(shared, at) {
            Due::Show => shared.show(frame, at),
            Due::Skip => {}
            Due::Dropped => return Ok(true),
        }
    }
    if shown == 0 {
        return Err("The video has no frame this decoder can read.".into());
    }
    Ok(false)
}

enum Due {
    Show,
    Skip,
    Dropped,
}

/// Paces one pass: by the sound's clock while it plays, else by the frames' own.
#[derive(Default)]
struct Pace {
    /// The own clock: when, and at which frame time, it started.
    wall: Option<(Instant, f64)>,
    /// When frames began waiting for the sound to start.
    waiting: Option<Instant>,
    shown: Option<Instant>,
}

impl Pace {
    /// Waits until the frame `at` seconds into the clip is due, or says to drop it.
    fn wait(&mut self, shared: &Shared, at: f64) -> Due {
        let mut state = shared.lock();
        loop {
            if state.dropped {
                return Due::Dropped;
            }
            let now = Instant::now();
            if !state.playing {
                // Paused: the clocks restart at this frame on resume.
                (self.wall, self.waiting) = (None, None);
                state = shared.changed.wait(state).unwrap_or_else(|e| e.into_inner());
                continue;
            }
            let waited = now.duration_since(*self.waiting.get_or_insert(now));
            let ahead = match state.sound {
                Sound::At(time, read) => {
                    self.wall = None;
                    let ahead = at - time - now.duration_since(read).as_secs_f64();
                    // Dropped to catch up, though a frame still shows every quarter second.
                    if ahead < -LATE && self.shown.is_some_and(|s| now.duration_since(s) < Duration::from_millis(250)) {
                        return Due::Skip;
                    }
                    ahead
                }
                Sound::Waiting if waited < SOUND_WAIT => {
                    state = shared.changed.wait_timeout(state, SOUND_WAIT - waited).unwrap_or_else(|e| e.into_inner()).0;
                    continue;
                }
                Sound::Waiting | Sound::Off => {
                    let (start, base) = *self.wall.get_or_insert((now, at));
                    at - base - now.duration_since(start).as_secs_f64()
                }
            };
            if ahead <= 0.0 {
                self.shown = Some(now);
                return Due::Show;
            }
            state = shared.changed.wait_timeout(state, Duration::from_secs_f64(ahead)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
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
        // The ended pass sets playing back to false, leaves the last frame up, and rewinds.
        let state = player.shared.changed.wait_while(player.shared.lock(), |s| s.playing).unwrap();
        assert!(state.at_start && state.status == Status::Ready);
    }

    #[test]
    fn high_profile_b_frames_decode_in_display_order() {
        for name in ["high-bframes.mp4", "high-bframes-fragmented.mp4"] {
            let clip = open(mp4::tests::fixture(name)).expect("FAILED: the clip opens");
            let mut decoder = h264::Decoder::reordering(&clip.track.params).expect("decoder");
            let unit = |s: &mp4::Sample| h264::length_nals(&clip.file[s.offset..s.offset + s.size], clip.track.length_size);
            let mut frames: Vec<h264::Frame> = clip.track.samples.iter().filter_map(|s| decoder.decode(&unit(s)).ok().flatten()).collect();
            frames.extend(decoder.finish().expect("FAILED: the held frames flush"));
            // Each of the fixture's frames is brighter than the one before.
            let green: Vec<u32> = frames.iter().map(|f| f.rgba.iter().skip(1).step_by(4).map(|g| u32::from(*g)).sum::<u32>() / (f.width * f.height)).collect();
            assert_eq!(green.len(), 12, "FAILED: {name}: every frame decodes: {green:?}");
            assert!(green.windows(2).all(|w| w[0] < w[1]), "FAILED: {name}: frames come back in display order: {green:?}");
        }
    }

    #[test]
    fn frames_follow_the_sound() {
        let player = SoftPlayer::new(None);
        let mut state = player.shared.lock();
        (state.playing, state.sound) = (true, Sound::At(5.0, Instant::now()));
        drop(state);
        let mut pace = Pace::default();
        assert!(matches!(pace.wait(&player.shared, 5.0), Due::Show), "FAILED: the frame at the sound's time shows");
        assert!(matches!(pace.wait(&player.shared, 1.0), Due::Skip), "FAILED: a frame seconds behind the sound is dropped");
        player.shared.lock().sound = Sound::Off;
        assert!(matches!(pace.wait(&player.shared, 1.0), Due::Show), "FAILED: without sound, frames keep their own clock from where they are");
    }

    #[test]
    fn what_is_not_h264_mp4_fails_with_the_reason() {
        let player = SoftPlayer::bytes(b"\x1a\x45\xdf\xa3webm".to_vec(), None);
        assert_eq!(player.status(), Status::Failed(mp4::UNSUPPORTED.into()));
    }
}
