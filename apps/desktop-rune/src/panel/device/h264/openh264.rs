//! OpenH264, built from its bundled source, fed Annex B.

use ::openh264::decoder::{DecodeOptions, DecodedYUV, Decoder as Inner, Flush};
use ::openh264::formats::YUVSource;

use super::{Frame, Params};

pub struct Decoder {
    inner: Inner,
    annexb: Vec<u8>,
    /// A file's B-frames: OpenH264 holds each picture until its turn in display order. The
    /// crate's default flush after every call would hand a held P-frame out ahead of them.
    reorder: bool,
}

fn push(out: &mut Vec<u8>, nal: &[u8]) {
    out.extend_from_slice(&[0, 0, 0, 1]);
    out.extend_from_slice(nal);
}

impl Decoder {
    pub fn new(params: &Params) -> Result<Self, String> {
        let mut inner = Inner::new().map_err(|e| e.to_string())?;
        let mut header = vec![];
        push(&mut header, &params.sps);
        push(&mut header, &params.pps);
        inner.decode(&header).map_err(|e| e.to_string())?;
        Ok(Decoder { inner, annexb: vec![], reorder: false })
    }

    pub fn reordering(params: &Params) -> Result<Self, String> {
        Self::new(params).map(|d| Decoder { reorder: true, ..d })
    }

    /// The pictures still held for reordering once the stream has ended.
    pub fn finish(&mut self) -> Result<Vec<Frame>, String> {
        Ok(self.inner.flush_remaining().map_err(|e| e.to_string())?.iter().map(frame).collect())
    }

    pub fn decode(&mut self, nals: &[&[u8]]) -> Result<Option<Frame>, String> {
        self.annexb.clear();
        for nal in nals {
            push(&mut self.annexb, nal);
        }
        let options = DecodeOptions::new().flush_after_decode(if self.reorder { Flush::NoFlush } else { Flush::Auto });
        Ok(self.inner.decode_with_options(&self.annexb, options).map_err(|e| e.to_string())?.as_ref().map(frame))
    }
}

fn frame(yuv: &DecodedYUV<'_>) -> Frame {
    let (width, height) = yuv.dimensions();
    let mut rgba = vec![0; width * height * 4];
    yuv.write_rgba8(&mut rgba);
    Frame { width: width as u32, height: height as u32, rgba }
}
