//! OpenH264, built from its bundled source, fed Annex B.

use ::openh264::decoder::Decoder as Inner;
use ::openh264::formats::YUVSource;

use super::{Frame, Params};

pub struct Decoder {
    inner: Inner,
    annexb: Vec<u8>,
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
        Ok(Decoder { inner, annexb: vec![] })
    }

    pub fn decode(&mut self, nals: &[&[u8]]) -> Result<Option<Frame>, String> {
        self.annexb.clear();
        for nal in nals {
            push(&mut self.annexb, nal);
        }
        let Some(yuv) = self.inner.decode(&self.annexb).map_err(|e| e.to_string())? else { return Ok(None) };
        let (width, height) = yuv.dimensions();
        let mut rgba = vec![0; width * height * 4];
        yuv.write_rgba8(&mut rgba);
        Ok(Some(Frame { width: width as u32, height: height as u32, rgba }))
    }
}
