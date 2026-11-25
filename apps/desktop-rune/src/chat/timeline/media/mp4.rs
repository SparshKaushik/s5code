//! Just enough ISO BMFF (MP4, and QuickTime's .mov) to play an H.264 track: its parameter sets,
//! and where each sample sits and when it shows, from the sample table and any movie fragments.

use crate::panel::device::h264::{self, Params};

pub struct Track {
    pub params: Params,
    /// Bytes in each NAL unit's length prefix.
    pub length_size: usize,
    /// Ticks per second of `Sample::pts`.
    pub timescale: u32,
    /// In decode order.
    pub samples: Vec<Sample>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sample {
    pub offset: usize,
    pub size: usize,
    /// When it shows, in `timescale` ticks from the first frame shown.
    pub pts: i64,
}

impl Track {
    /// Every sample's presentation time, earliest first: the order a decoder hands frames back.
    pub fn display_order(&self) -> Vec<i64> {
        let mut order: Vec<i64> = self.samples.iter().map(|s| s.pts).collect();
        order.sort_unstable();
        order
    }
}

/// More samples than this is a damaged table, not a video (about 39 hours at 30 frames a second).
const MAX_SAMPLES: usize = 1 << 22;
pub const UNSUPPORTED: &str = "Only MP4 and QuickTime video in H.264 plays here.";

fn u32_at(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

fn u64_at(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at.checked_add(8)?)?.try_into().ok()?))
}

fn u16_at(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at.checked_add(2)?)?.try_into().ok()?))
}

/// The boxes laid end to end in `data`: each one's type, where it starts, and its body.
fn boxes(data: &[u8]) -> Vec<([u8; 4], usize, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while let (Some(size), Some(kind)) = (u32_at(data, at), data.get(at + 4..at + 8)) {
        let (header, size) = match size {
            1 => match u64_at(data, at + 8) {
                Some(large) => (16, large),
                None => break,
            },
            0 => (8, (data.len() - at) as u64),
            n => (8, n as u64),
        };
        let Some(end) = usize::try_from(size).ok().filter(|s| *s >= header).and_then(|s| at.checked_add(s)).filter(|e| *e <= data.len()) else { break };
        let Ok(kind) = <[u8; 4]>::try_from(kind) else { break };
        out.push((kind, at, &data[at + header..end]));
        at = end;
    }
    out
}

fn children(data: &[u8]) -> Vec<([u8; 4], &[u8])> {
    boxes(data).into_iter().map(|(kind, _, body)| (kind, body)).collect()
}

fn child<'a>(data: &'a [u8], kind: &[u8; 4]) -> Option<&'a [u8]> {
    children(data).into_iter().find(|(k, _)| k == kind).map(|(_, body)| body)
}

/// A full box's body past its version and flags.
fn full(body: &[u8]) -> Option<&[u8]> {
    body.get(4..)
}

/// A table of `count` fixed-size rows after a full box's entry count.
fn rows(body: &[u8], width: usize) -> Option<(&[u8], usize)> {
    let body = full(body)?;
    let count = u32_at(body, 0)? as usize;
    if count > MAX_SAMPLES || body.len() < 4 + count.checked_mul(width)? {
        return None;
    }
    Some((&body[4..], count))
}

/// The first H.264 video track of an MP4 or QuickTime file.
pub fn parse(file: &[u8]) -> Result<Track, String> {
    let moov = child(file, b"moov").ok_or(UNSUPPORTED)?;
    children(moov)
        .into_iter()
        .filter(|(kind, _)| kind == b"trak")
        .find_map(|(_, trak)| track(file, moov, trak))
        .ok_or_else(|| UNSUPPORTED.to_owned())?
}

/// Whether the file has a sound track, which Rune's audio player then reads.
pub fn has_sound(file: &[u8]) -> bool {
    let handler = |trak: &[u8]| Some(child(child(trak, b"mdia")?, b"hdlr")?.get(8..12)? == b"soun");
    child(file, b"moov").is_some_and(|moov| children(moov).into_iter().any(|(kind, trak)| kind == *b"trak" && handler(trak) == Some(true)))
}

/// `None` when the track isn't H.264 video; an error when it is but can't be read.
fn track(file: &[u8], moov: &[u8], trak: &[u8]) -> Option<Result<Track, String>> {
    let mdia = child(trak, b"mdia")?;
    if child(mdia, b"hdlr")?.get(8..12)? != b"vide" {
        return None;
    }
    let stbl = child(child(mdia, b"minf")?, b"stbl")?;
    let (kind, entry) = *children(full(child(stbl, b"stsd")?)?.get(4..)?).first()?;
    if kind != *b"avc1" && kind != *b"avc3" {
        return None;
    }
    // A VisualSampleEntry's fields take 78 bytes before its child boxes.
    let (params, length_size) = h264::avcc_record(child(entry.get(78..)?, b"avcC")?)?;
    let samples = || {
        let (mut samples, end) = table(stbl)?;
        if let Some(mvex) = child(moov, b"mvex") {
            fragments(file, mvex, track_id(trak)?, end, &mut samples)?;
        }
        if samples.iter().any(|s| s.offset.checked_add(s.size).is_none_or(|end| end > file.len())) {
            return None;
        }
        let first = samples.iter().map(|s| s.pts).min().unwrap_or(0);
        samples.iter_mut().for_each(|s| s.pts -= first);
        Some(samples)
    };
    Some(samples().map(|samples| Track { params, length_size, timescale: timescale(mdia).unwrap_or(1000), samples }).ok_or_else(|| "The video's sample table is damaged.".to_owned()))
}

fn track_id(trak: &[u8]) -> Option<u32> {
    let tkhd = child(trak, b"tkhd")?;
    u32_at(tkhd, if tkhd.first()? == &1 { 20 } else { 12 })
}

fn timescale(mdia: &[u8]) -> Option<u32> {
    let mdhd = child(mdia, b"mdhd")?;
    let at = if mdhd.first()? == &1 { 20 } else { 12 };
    u32_at(mdhd, at).filter(|t| *t > 0)
}

/// Each sample's place in the file and its presentation time, from the sample table, and the
/// decode time after its last sample.
fn table(stbl: &[u8]) -> Option<(Vec<Sample>, i64)> {
    let stsz = full(child(stbl, b"stsz")?)?;
    let (fixed, count) = (u32_at(stsz, 0)?, u32_at(stsz, 4)? as usize);
    if count > MAX_SAMPLES || (fixed == 0 && stsz.len() < 8 + count * 4) {
        return None;
    }
    let size = |i: usize| if fixed != 0 { Some(fixed as usize) } else { u32_at(stsz, 8 + i * 4).map(|s| s as usize) };
    let chunks: Vec<u64> = if let Some(stco) = child(stbl, b"stco") {
        let (t, n) = rows(stco, 4)?;
        (0..n).map(|i| u32_at(t, i * 4).map(u64::from)).collect::<Option<_>>()?
    } else {
        let (t, n) = rows(child(stbl, b"co64")?, 8)?;
        (0..n).map(|i| u64_at(t, i * 8)).collect::<Option<_>>()?
    };
    let (stsc, runs) = rows(child(stbl, b"stsc")?, 12)?;
    let run = |i: usize| Some((u32_at(stsc, i * 12)? as usize, u32_at(stsc, i * 12 + 4)? as usize));

    let mut offsets = Vec::with_capacity(count);
    for r in 0..runs {
        let (first, per_chunk) = run(r)?;
        let last = if r + 1 < runs { run(r + 1)?.0 } else { chunks.len() + 1 };
        for chunk in first.max(1)..last.min(chunks.len() + 1) {
            let mut at = usize::try_from(chunks[chunk - 1]).ok()?;
            for _ in 0..per_chunk {
                if offsets.len() == count {
                    break;
                }
                let s = size(offsets.len())?;
                offsets.push((at, s));
                at = at.checked_add(s)?;
            }
        }
    }
    if offsets.len() != count {
        return None;
    }

    let (stts, deltas) = rows(child(stbl, b"stts")?, 8)?;
    let mut dts = Vec::with_capacity(count);
    let mut t = 0i64;
    for i in 0..deltas {
        let (n, delta) = (u32_at(stts, i * 8)? as usize, u32_at(stts, i * 8 + 4)? as i64);
        for _ in 0..n.min(count - dts.len()) {
            dts.push(t);
            t += delta;
        }
    }
    dts.resize(count, t);
    let mut shift = vec![0i64; count];
    if let Some((ctts, n)) = child(stbl, b"ctts").and_then(|b| rows(b, 8)) {
        let mut at = 0;
        for i in 0..n {
            // Signed in version 1, and read that way in version 0 too, as players do.
            let (k, offset) = (u32_at(ctts, i * 8)? as usize, u32_at(ctts, i * 8 + 4)? as i32 as i64);
            for slot in shift.iter_mut().skip(at).take(k) {
                *slot = offset;
            }
            at += k;
        }
    }
    Some((offsets.into_iter().enumerate().map(|(i, (offset, size))| Sample { offset, size, pts: dts[i] + shift[i] }).collect(), t))
}

/// Appends track `id`'s samples from each movie fragment (`moof`), decoding on from `dts`.
fn fragments(file: &[u8], mvex: &[u8], id: u32, mut dts: i64, out: &mut Vec<Sample>) -> Option<()> {
    // trex: the track's defaults, after its ID and sample description index.
    let trex = children(mvex).into_iter().filter(|(k, _)| k == b"trex").filter_map(|(_, b)| full(b)).find(|b| u32_at(b, 0) == Some(id));
    let default = |at: usize| trex.and_then(|b| u32_at(b, at)).unwrap_or(0);
    for (_, moof_at, moof) in boxes(file).into_iter().filter(|(k, _, _)| k == b"moof") {
        for (_, traf) in children(moof).into_iter().filter(|(k, _)| k == b"traf") {
            let tfhd = child(traf, b"tfhd")?;
            let (flags, tfhd) = (u32_at(tfhd, 0)? & 0xff_ffff, full(tfhd)?);
            if u32_at(tfhd, 0)? != id {
                continue;
            }
            let mut at = 4;
            let mut field = |set: bool, width: usize| {
                let here = at;
                at += if set { width } else { 0 };
                set.then_some(here)
            };
            let base = match field(flags & 0x1 != 0, 8) {
                Some(here) => usize::try_from(u64_at(tfhd, here)?).ok()?,
                None => moof_at,
            };
            field(flags & 0x2 != 0, 4);
            let duration = field(flags & 0x8 != 0, 4).map_or(Some(default(8)), |here| u32_at(tfhd, here))?;
            let size = field(flags & 0x10 != 0, 4).map_or(Some(default(12)), |here| u32_at(tfhd, here))?;
            if let Some(tfdt) = child(traf, b"tfdt") {
                dts = if tfdt.first()? == &1 { i64::try_from(u64_at(tfdt, 4)?).ok()? } else { i64::from(u32_at(tfdt, 4)?) };
            }
            let mut pos = base;
            for (_, trun) in children(traf).into_iter().filter(|(k, _)| k == b"trun") {
                let (flags, trun) = (u32_at(trun, 0)? & 0xff_ffff, full(trun)?);
                let count = u32_at(trun, 0)? as usize;
                let mut at = 4;
                if flags & 0x1 != 0 {
                    pos = base.checked_add_signed(u32_at(trun, at)? as i32 as isize)?;
                    at += 4;
                }
                if flags & 0x4 != 0 {
                    at += 4;
                }
                let width = [0x100, 0x200, 0x400, 0x800].iter().filter(|f| flags & *f != 0).count() * 4;
                if out.len() + count > MAX_SAMPLES || trun.len() < at + count.checked_mul(width)? {
                    return None;
                }
                for _ in 0..count {
                    let mut next = |set: u32| {
                        let v = (flags & set != 0).then(|| u32_at(trun, at));
                        at += if flags & set != 0 { 4 } else { 0 };
                        v.flatten()
                    };
                    let (d, s, _, shift) = (next(0x100).unwrap_or(duration), next(0x200).unwrap_or(size), next(0x400), next(0x800).map_or(0, |c| c as i32 as i64));
                    out.push(Sample { offset: pos, size: s as usize, pts: dts + shift });
                    pos = pos.checked_add(s as usize)?;
                    dts += i64::from(d);
                }
            }
        }
    }
    Some(())
}

/// A file holding `units`, Annex B access units of one H.264 stream, at `fps` frames a second.
/// Scenes and tests make clips with it.
#[cfg(any(test, target_os = "linux"))]
pub fn write(units: &[Vec<u8>], width: u16, height: u16, fps: u32) -> Vec<u8> {
    fn boxed(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = ((body.len() + 8) as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out
    }
    fn full_box(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        boxed(kind, &[&[0, 0, 0, 0][..], body].concat())
    }
    let all: Vec<Vec<&[u8]>> = units.iter().map(|u| h264::annexb_nals(u)).collect();
    let params = all.iter().find_map(|nals| h264::params_in(nals)).unwrap_or(Params { sps: vec![], pps: vec![] });
    let samples: Vec<Vec<u8>> = all
        .iter()
        .map(|nals| nals.iter().filter(|n| !matches!(n.first().map(|b| b & 0x1f), Some(7 | 8))).flat_map(|n| [&(n.len() as u32).to_be_bytes()[..], *n].concat()).collect())
        .collect();
    let mut avcc = vec![1, params.sps.get(1).copied().unwrap_or(0), 0, params.sps.get(3).copied().unwrap_or(0), 0xff, 0xe1];
    avcc.extend_from_slice(&(params.sps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(&params.sps);
    avcc.push(1);
    avcc.extend_from_slice(&(params.pps.len() as u16).to_be_bytes());
    avcc.extend_from_slice(&params.pps);
    let mut visual = vec![0u8; 78];
    visual[7] = 1;
    visual[24..26].copy_from_slice(&width.to_be_bytes());
    visual[26..28].copy_from_slice(&height.to_be_bytes());
    visual[74..76].copy_from_slice(&24u16.to_be_bytes());
    visual[76..78].copy_from_slice(&(-1i16).to_be_bytes());
    let entry = boxed(b"avc1", &[visual, boxed(b"avcC", &avcc)].concat());
    let n = samples.len() as u32;
    let ftyp = boxed(b"ftyp", b"isom\0\0\x02\0isomavc1");
    let mdat_at = ftyp.len() + 8;
    let mut stsz = 0u32.to_be_bytes().to_vec();
    stsz.extend_from_slice(&n.to_be_bytes());
    samples.iter().for_each(|s| stsz.extend_from_slice(&(s.len() as u32).to_be_bytes()));
    let stbl = [
        full_box(b"stsd", &[&1u32.to_be_bytes()[..], &entry[..]].concat()),
        full_box(b"stts", &[1u32, n, 1].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>()),
        full_box(b"stsc", &[1u32, 1, n, 1].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>()),
        full_box(b"stsz", &stsz),
        full_box(b"stco", &[1u32, mdat_at as u32].iter().flat_map(|v| v.to_be_bytes()).collect::<Vec<_>>()),
    ]
    .concat();
    let mut mdhd = vec![0u8; 20];
    mdhd[8..12].copy_from_slice(&fps.to_be_bytes());
    let mut hdlr = vec![0u8; 20];
    hdlr[4..8].copy_from_slice(b"vide");
    let mdia = boxed(b"mdia", &[full_box(b"mdhd", &mdhd), full_box(b"hdlr", &hdlr), boxed(b"minf", &boxed(b"stbl", &stbl))].concat());
    let moov = boxed(b"moov", &boxed(b"trak", &mdia));
    [ftyp, boxed(b"mdat", &samples.concat()), moov].concat()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn unit(nals: &[&[u8]]) -> Vec<u8> {
        nals.iter().flat_map(|n| [&[0, 0, 0, 1][..], n].concat()).collect()
    }

    #[test]
    fn a_written_clip_reads_back_its_samples_in_order() {
        let (sps, pps) = (&[0x67, 0x42, 0xc0, 0x1e, 0x01][..], &[0x68, 0xce, 0x3c, 0x80][..]);
        let units = [unit(&[sps, pps, &[0x65, 1, 2, 3]]), unit(&[&[0x41, 4, 5]]), unit(&[&[0x41, 6]])];
        let file = write(&units, 64, 48, 30);
        let track = parse(&file).expect("FAILED: the clip parses");
        assert_eq!((track.params.sps.as_slice(), track.params.pps.as_slice(), track.length_size, track.timescale), (sps, pps, 4, 30));
        let bodies: Vec<Vec<&[u8]>> = track.samples.iter().map(|s| h264::length_nals(&file[s.offset..s.offset + s.size], 4)).collect();
        assert_eq!(bodies, vec![vec![&[0x65, 1, 2, 3][..]], vec![&[0x41, 4, 5][..]], vec![&[0x41, 6][..]]], "FAILED: parameter sets live in avcC, not the samples");
        assert_eq!(track.samples.iter().map(|s| s.pts).collect::<Vec<_>>(), vec![0, 1, 2]);
        assert!(!has_sound(&file));
    }

    pub(crate) fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{}/fixtures/video/{name}", env!("CARGO_MANIFEST_DIR"))).expect("fixture")
    }

    #[test]
    fn a_fragmented_clip_reads_like_its_plain_copy() {
        // ffmpeg's libx264, High profile with two B-frames and a keyframe every 6, and AAC sound;
        // once plain, once with `-movflags frag_keyframe+empty_moov+default_base_moof`.
        let read = |name: &str| {
            let file = fixture(name);
            let track = parse(&file).unwrap_or_else(|e| panic!("FAILED: {name} parses: {e}"));
            let samples: Vec<(Vec<u8>, i64)> = track.samples.iter().map(|s| (file[s.offset..s.offset + s.size].to_vec(), s.pts)).collect();
            (track.timescale, track.params, has_sound(&file), samples)
        };
        let (plain, fragmented) = (read("high-bframes.mp4"), read("high-bframes-fragmented.mp4"));
        let pts: Vec<i64> = plain.3.iter().map(|(_, pts)| *pts).collect();
        assert_eq!(pts, [0, 3, 1, 2, 5, 4, 6, 9, 7, 8, 11, 10].map(|n| n * 1024), "FAILED: B-frames show before the frame decoded ahead of them");
        assert_eq!((plain.0, plain.2), (12288, true));
        assert!(plain == fragmented, "FAILED: the fragments hold the same samples, at the same times");
    }

    #[test]
    fn what_cannot_play_says_so() {
        assert_eq!(parse(b"not a video").err().as_deref(), Some(UNSUPPORTED));
        let trackless = [&20u32.to_be_bytes()[..], b"moov", &12u32.to_be_bytes(), b"mvex", &[0; 4]].concat();
        assert_eq!(parse(&trackless).err().as_deref(), Some(UNSUPPORTED));
        let units = [unit(&[&[0x67, 0x42], &[0x68, 0xce], &[0x65, 1]])];
        let mut file = write(&units, 2, 2, 30);
        // The chunk now starts at the file's end, so its sample runs past it.
        let at = file.windows(4).position(|w| w == b"stco").expect("stco") + 12;
        let end = (file.len() as u32).to_be_bytes();
        file[at..at + 4].copy_from_slice(&end);
        assert_eq!(parse(&file).err().as_deref(), Some("The video's sample table is damaged."));
    }
}
