//! H.264 for the device streams web decodes with WebCodecs: serve-sim's AVCC feed (iOS and
//! Simlock) and serve-emu's SEMU packets (Android). VideoToolbox decodes on macOS, OpenH264
//! on Linux.

#[cfg(target_os = "linux")]
#[path = "h264/openh264.rs"]
mod backend;
#[cfg(target_os = "macos")]
#[path = "h264/videotoolbox.rs"]
mod backend;
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod backend {
    pub struct Decoder;
    impl Decoder {
        pub fn new(_: &super::Params) -> Result<Self, String> {
            Err("No H.264 decoder on this platform.".into())
        }
        pub fn decode(&mut self, _: &[&[u8]]) -> Result<Option<super::Frame>, String> {
            Ok(None)
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Params {
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
}

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

const SPS: u8 = 7;
const PPS: u8 = 8;
const IDR: u8 = 5;

fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |b| b & 0x1f)
}

/// NAL units of an Annex B access unit, without their start codes.
pub fn annexb_nals(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = vec![];
    let mut i = 0;
    while i + 2 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &start) in starts.iter().enumerate() {
        let mut end = starts.get(k + 1).map_or(data.len(), |next| next - 3);
        // A four-byte start code's leading zero, and trailing zero bytes, aren't the NAL's.
        while end > start && data[end - 1] == 0 {
            end -= 1;
        }
        if end > start {
            out.push(&data[start..end]);
        }
    }
    out
}

/// NAL units of an AVCC sample: each behind a big-endian length of `size` bytes.
pub fn length_nals(data: &[u8], size: usize) -> Vec<&[u8]> {
    let mut out = vec![];
    let mut at = 0;
    while size > 0 && at + size <= data.len() {
        let len = data[at..at + size].iter().fold(0usize, |n, b| (n << 8) | *b as usize);
        at += size;
        let Some(nal) = data.get(at..at + len) else { break };
        if !nal.is_empty() {
            out.push(nal);
        }
        at += len;
    }
    out
}

/// An `AVCDecoderConfigurationRecord`: the first SPS and PPS, and the NAL length size.
pub fn avcc_record(record: &[u8]) -> Option<(Params, usize)> {
    let length_size = (*record.get(4)? as usize & 0x03) + 1;
    let mut at = 5;
    let mut take = |count_mask: u8| -> Option<Vec<Vec<u8>>> {
        let count = (*record.get(at)? & count_mask) as usize;
        at += 1;
        let mut sets = vec![];
        for _ in 0..count {
            let len = u16::from_be_bytes([*record.get(at)?, *record.get(at + 1)?]) as usize;
            sets.push(record.get(at + 2..at + 2 + len)?.to_vec());
            at += 2 + len;
        }
        Some(sets)
    };
    let sps = take(0x1f)?.into_iter().next()?;
    let pps = take(0xff)?.into_iter().next()?;
    Some((Params { sps, pps }, length_size))
}

/// The SPS and PPS an access unit carries, if it carries both.
pub fn params_in(nals: &[&[u8]]) -> Option<Params> {
    let sps = nals.iter().find(|n| nal_type(n) == SPS)?;
    let pps = nals.iter().find(|n| nal_type(n) == PPS)?;
    Some(Params { sps: sps.to_vec(), pps: pps.to_vec() })
}

pub fn is_key(nals: &[&[u8]]) -> bool {
    nals.iter().any(|n| nal_type(n) == IDR)
}

/// serve-emu's `SEMU` packet: a 16-byte header (magic, version 1, flags, pts) before an Annex B
/// access unit. Anything else is a bare access unit.
pub fn semu(packet: &[u8]) -> (&[u8], Option<bool>) {
    match packet {
        [b'S', b'E', b'M', b'U', 1, flags, ..] if packet.len() > 16 => (&packet[16..], Some(flags & 1 != 0)),
        _ => (packet, None),
    }
}

/// serve-sim's `stream.avcc`: frames of a 4-byte big-endian length, a tag and its payload.
#[derive(Default)]
pub struct Avcc {
    buf: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum Chunk {
    Description(Vec<u8>),
    Keyframe(Vec<u8>),
    Delta(Vec<u8>),
    /// A JPEG of the screen to show until the first decoded frame.
    Seed(Vec<u8>),
}

impl Avcc {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Chunk> {
        self.buf.extend_from_slice(bytes);
        let mut out = vec![];
        let mut at = 0;
        while self.buf.len() - at >= 4 {
            let len = u32::from_be_bytes([self.buf[at], self.buf[at + 1], self.buf[at + 2], self.buf[at + 3]]) as usize;
            if self.buf.len() - at - 4 < len {
                break;
            }
            if len >= 1 {
                let payload = self.buf[at + 5..at + 4 + len].to_vec();
                match self.buf[at + 4] {
                    1 => out.push(Chunk::Description(payload)),
                    2 => out.push(Chunk::Keyframe(payload)),
                    3 => out.push(Chunk::Delta(payload)),
                    4 => out.push(Chunk::Seed(payload)),
                    _ => {}
                }
            }
            at += 4 + len;
        }
        self.buf.drain(..at);
        out
    }

    /// Bytes waiting for the rest of their frame.
    pub fn pending(&self) -> usize {
        self.buf.len()
    }
}

/// One platform decoder; dropping it releases the session.
pub struct Decoder(backend::Decoder);

impl Decoder {
    pub fn new(params: &Params) -> Result<Self, String> {
        backend::Decoder::new(params).map(Decoder)
    }

    /// One access unit's NAL units, without start codes or lengths; a frame when one is ready.
    pub fn decode(&mut self, nals: &[&[u8]]) -> Result<Option<Frame>, String> {
        self.0.decode(nals)
    }
}

/// The decoder for one feed: reconfigured when the parameter sets change, as after a rotation,
/// and fed from the next keyframe on.
#[derive(Default)]
pub struct Feed {
    decoder: Option<(Params, Decoder)>,
    awaiting_key: bool,
}

impl Feed {
    pub fn configured(&self) -> bool {
        self.decoder.is_some()
    }

    /// Nothing will draw until a keyframe arrives.
    pub fn waiting(&self) -> bool {
        self.decoder.is_none() || self.awaiting_key
    }

    pub fn configure(&mut self, params: Params) -> Result<(), String> {
        if self.decoder.as_ref().is_some_and(|(p, _)| *p == params) {
            return Ok(());
        }
        self.decoder = None;
        let decoder = Decoder::new(&params)?;
        self.decoder = Some((params, decoder));
        self.awaiting_key = true;
        Ok(())
    }

    pub fn reset(&mut self) {
        self.decoder = None;
    }

    pub fn unit(&mut self, nals: &[&[u8]]) -> Result<Option<Frame>, String> {
        if let Some(params) = params_in(nals) {
            self.configure(params)?;
        }
        let Some((_, decoder)) = self.decoder.as_mut() else { return Ok(None) };
        if self.awaiting_key {
            if !is_key(nals) {
                return Ok(None);
            }
            self.awaiting_key = false;
        }
        decoder.decode(nals)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three 64×48 frames from ffmpeg's libx264 (baseline): red over blue.
    const CLIP: &[u8] = include_bytes!("../../../fixtures/h264/split-64x48.h264");

    #[test]
    fn annex_b_and_avcc_framing_split_into_nal_units() {
        let nals = annexb_nals(CLIP);
        let types: Vec<u8> = nals.iter().map(|n| nal_type(n)).collect();
        assert_eq!(types, vec![SPS, PPS, 6, IDR, 1, 1]);
        assert!(nals.iter().all(|n| n.last() != Some(&0)), "start-code zeros stay out of the NAL");
        let params = params_in(&nals).unwrap();
        assert!(is_key(&nals[..4]) && !is_key(&nals[4..]));

        let mut sample = vec![];
        for nal in &nals[3..5] {
            sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            sample.extend_from_slice(nal);
        }
        assert_eq!(length_nals(&sample, 4), nals[3..5].to_vec());
        assert_eq!(length_nals(&sample[..sample.len() - 1], 4), nals[3..4].to_vec(), "a cut sample keeps whole NALs");

        let mut record = vec![1, params.sps[1], params.sps[2], params.sps[3], 0xff, 0xe1];
        record.extend_from_slice(&(params.sps.len() as u16).to_be_bytes());
        record.extend_from_slice(&params.sps);
        record.push(1);
        record.extend_from_slice(&(params.pps.len() as u16).to_be_bytes());
        record.extend_from_slice(&params.pps);
        assert_eq!(avcc_record(&record), Some((params, 4)));
        assert_eq!(avcc_record(&record[..8]), None);

        let mut packet = b"SEMU\x01\x01\0\0\0\0\0\0\0\0\0\x07".to_vec();
        packet.extend_from_slice(CLIP);
        assert_eq!(semu(&packet), (CLIP, Some(true)));
        assert_eq!(semu(CLIP), (CLIP, None));
    }

    #[test]
    fn avcc_frames_wait_for_their_whole_payload() {
        let mut stream = vec![];
        for (tag, payload) in [(1u8, &b"desc"[..]), (2, b"key"), (9, b"skip"), (4, b"jpeg")] {
            stream.extend_from_slice(&(payload.len() as u32 + 1).to_be_bytes());
            stream.push(tag);
            stream.extend_from_slice(payload);
        }
        let mut avcc = Avcc::default();
        assert_eq!(avcc.push(&stream[..10]), vec![Chunk::Description(b"desc".to_vec())]);
        assert_eq!(avcc.pending(), 1);
        assert_eq!(avcc.push(&stream[10..]), vec![Chunk::Keyframe(b"key".to_vec()), Chunk::Seed(b"jpeg".to_vec())]);
        assert_eq!(avcc.pending(), 0);
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn the_platform_decoder_draws_the_clip() {
        let nals = annexb_nals(CLIP);
        let mut feed = Feed::default();
        assert!(feed.unit(&nals[4..5]).unwrap().is_none(), "nothing decodes before parameter sets");
        let mut frames = vec![];
        for unit in [&nals[..4], &nals[4..5], &nals[5..6]] {
            frames.extend(feed.unit(unit).unwrap());
        }
        assert!(!frames.is_empty(), "the keyframe draws");
        for frame in &frames {
            assert_eq!((frame.width, frame.height), (64, 48));
            assert_eq!(frame.rgba.len(), 64 * 48 * 4);
            let px = |x: usize, y: usize| &frame.rgba[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4];
            let (top, bottom) = (px(32, 6), px(32, 42));
            assert!(top[0] > 200 && top[2] < 60 && top[3] == 255, "red on top, RGBA order: {top:?}");
            assert!(bottom[2] > 200 && bottom[0] < 60, "blue below: {bottom:?}");
        }
    }
}
