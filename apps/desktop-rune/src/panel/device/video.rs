//! The H.264 feeds: serve-sim's `stream.avcc` over HTTP (iOS and Simlock) and serve-emu's
//! socket (Android), which also carries that emulator's input.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::Pixels;
use tungstenite::Message;

use super::h264::{self, Avcc, Chunk, Feed};
use super::{Shared, input};

/// Web's `FIRST_FRAME_TIMEOUT_MS`: an AVCC body can stay open after its helper stops producing.
const STALL: Duration = Duration::from_secs(15);
const KEYFRAME_EVERY: Duration = Duration::from_secs(1);

pub enum Failure {
    /// This platform can't decode the stream; iOS falls back to MJPEG.
    Decoder(String),
    /// serve-sim is locked to WebRTC (409), which this app can't receive.
    WebRtc,
    /// The stream failed or ended; the panel offers Reconnect.
    Stream,
}

pub fn paint(pixels: &Pixels, shared: &Mutex<Shared>, wake: &dyn Fn(), width: u32, height: u32, rgba: Vec<u8>) {
    pixels.set(width, height, rgba);
    shared.lock().unwrap_or_else(|e| e.into_inner()).frames += 1;
    wake();
}

fn paint_jpeg(pixels: &Pixels, shared: &Mutex<Shared>, wake: &dyn Fn(), jpeg: &[u8]) {
    if let Ok(image) = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg) {
        let rgba = image.to_rgba8();
        paint(pixels, shared, wake, rgba.width(), rgba.height(), rgba.into_raw());
    }
}

/// serve-sim's helper only accepts input and reports its screen once capture runs, which the AVCC
/// stream doesn't reliably start and touching the MJPEG endpoint does (web's `primeIosHelper`).
pub fn prime(mjpeg: &str) {
    let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build();
    if let Ok(response) = agent.get(mjpeg).call() {
        let _ = response.into_reader().read(&mut [0u8; 1024]);
    }
}

/// Reads `stream.avcc` until the panel lets go, reconnecting when it stalls.
pub fn read_avcc(url: &str, pixels: &Pixels, shared: &Mutex<Shared>, stop: &AtomicBool, wake: &dyn Fn()) -> Result<(), Failure> {
    let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(STALL).build();
    let mut decoded = false;
    while !stop.load(Ordering::Relaxed) {
        let response = match agent.get(url).call() {
            Ok(response) => response,
            Err(ureq::Error::Status(409, _)) => return Err(Failure::WebRtc),
            Err(_) => return Err(Failure::Stream),
        };
        let mut reader = response.into_reader();
        let (mut avcc, mut feed, mut length_size) = (Avcc::default(), Feed::default(), 4);
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            let n = match reader.read(&mut chunk) {
                Ok(0) => return Err(Failure::Stream),
                Ok(n) => n,
                // Stalled: a new request starts a fresh capture with a keyframe.
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => break,
                Err(_) => return Err(Failure::Stream),
            };
            for chunk in avcc.push(&chunk[..n]) {
                match chunk {
                    Chunk::Seed(jpeg) if !decoded => paint_jpeg(pixels, shared, wake, &jpeg),
                    Chunk::Seed(_) => {}
                    Chunk::Description(record) => {
                        let (params, size) = h264::avcc_record(&record).ok_or_else(|| Failure::Decoder("Unreadable H.264 description.".into()))?;
                        length_size = size;
                        feed.configure(params).map_err(Failure::Decoder)?;
                    }
                    Chunk::Keyframe(sample) | Chunk::Delta(sample) => match feed.unit(&h264::length_nals(&sample, length_size)) {
                        Ok(Some(frame)) => {
                            decoded = true;
                            paint(pixels, shared, wake, frame.width, frame.height, frame.rgba);
                        }
                        Ok(None) => {}
                        // A corrupt sample: wait for the next keyframe on a fresh decoder.
                        Err(_) => feed.reset(),
                    },
                }
            }
            if avcc.pending() > 64 << 20 {
                return Err(Failure::Stream);
            }
        }
    }
    Ok(())
}

/// serve-emu's socket for one emulator: video in, input out, until the panel lets go.
#[allow(clippy::too_many_arguments)]
pub fn run_semu(base: &str, auth: &s5_net::Auth, query: &str, rx: &Receiver<Message>, input: &Mutex<input::State>, pixels: &Pixels, shared: &Mutex<Shared>, stop: &AtomicBool, wake: &dyn Fn()) -> Result<(), String> {
    let mut socket = s5_net::api_socket(base, auth, "/api/device-hub/vendor/serve-emu/ws", query, Duration::from_millis(15))?;
    input.lock().unwrap_or_else(|e| e.into_inner()).admitted = true;
    wake();
    let (mut feed, mut asked) = (Feed::default(), None::<Instant>);
    let keyframe = |socket: &mut tungstenite::WebSocket<_>, asked: &mut Option<Instant>| {
        if asked.is_none_or(|at| at.elapsed() >= KEYFRAME_EVERY) {
            *asked = Some(Instant::now());
            let _ = socket.send(Message::Text(r#"{"type":"reset-video","ack":false}"#.into()));
        }
    };
    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        match rx.try_recv() {
            Ok(message) => {
                if socket.send(message).is_err() {
                    break Err("The device connection closed. Reconnect to try again.".into());
                }
                continue;
            }
            Err(TryRecvError::Disconnected) => break Ok(()),
            Err(TryRecvError::Empty) => {}
        }
        match socket.read() {
            Ok(Message::Binary(packet)) => {
                let (unit, _) = h264::semu(&packet);
                let nals = h264::annexb_nals(unit);
                let fresh = !feed.configured();
                match feed.unit(&nals) {
                    Ok(Some(frame)) => paint(pixels, shared, wake, frame.width, frame.height, frame.rgba),
                    Ok(None) if feed.waiting() && !h264::is_key(&nals) => keyframe(&mut socket, &mut asked),
                    Ok(None) => {}
                    Err(e) if fresh && !feed.configured() => break Err(e),
                    Err(_) => {
                        feed.reset();
                        keyframe(&mut socket, &mut asked);
                    }
                }
            }
            // Rotation restarts the encoder, which then sends a fresh keyframe unprompted.
            Ok(Message::Text(text)) if text.contains("\"video-session\"") => feed.reset(),
            Ok(Message::Close(frame)) => break Err(frame.map(|f| f.reason.to_string()).filter(|r| !r.is_empty()).unwrap_or_else(|| "The device connection closed. Reconnect to try again.".into())),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break Err("The device connection closed. Reconnect to try again.".into()),
        }
    };
    let _ = socket.close(None);
    let _ = socket.flush();
    input.lock().unwrap_or_else(|e| e.into_inner()).admitted = false;
    result
}

/// Starts Android's feed thread; its errors show on the panel with Reconnect.
#[allow(clippy::too_many_arguments)]
pub fn spawn_semu(base: String, auth: s5_net::Auth, query: String, rx: Receiver<Message>, input: Arc<Mutex<input::State>>, pixels: Pixels, shared: Arc<Mutex<Shared>>, stop: Arc<AtomicBool>, wake: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(e) = run_semu(&base, &auth, &query, &rx, &input, &pixels, &shared, &stop, &wake)
            && !stop.load(Ordering::Relaxed)
        {
            shared.lock().unwrap_or_else(|e| e.into_inner()).failed = Some(e);
            wake();
        }
    });
}
