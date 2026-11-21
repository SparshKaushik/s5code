//! VideoToolbox through its C API, as `desktop/macos.rs` reaches AppKit, so no new crates: a
//! decompression session per parameter set, decoding synchronously into BGRA pixel buffers.

use std::ffi::c_void;
use std::ptr::{null, null_mut};

use super::{Frame, Params};

type Ref = *const c_void;
type Status = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}

type OutputCallback = extern "C" fn(refcon: *mut c_void, frame_ref: *mut c_void, status: Status, flags: u32, image: Ref, pts: CMTime, duration: CMTime);

#[repr(C)]
struct OutputCallbackRecord {
    callback: OutputCallback,
    refcon: *mut c_void,
}

#[link(name = "CoreFoundation", kind = "framework")]
#[link(name = "CoreMedia", kind = "framework")]
#[link(name = "CoreVideo", kind = "framework")]
#[link(name = "VideoToolbox", kind = "framework")]
unsafe extern "C" {
    static kCFAllocatorNull: Ref;
    static kCFTypeDictionaryKeyCallBacks: u8;
    static kCFTypeDictionaryValueCallBacks: u8;
    static kCVPixelBufferPixelFormatTypeKey: Ref;
    fn CFRelease(cf: Ref);
    fn CFNumberCreate(allocator: Ref, kind: isize, value: *const c_void) -> Ref;
    fn CFDictionaryCreate(allocator: Ref, keys: *const Ref, values: *const Ref, count: isize, key_callbacks: *const c_void, value_callbacks: *const c_void) -> Ref;
    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(allocator: Ref, count: usize, pointers: *const *const u8, sizes: *const usize, nal_length_size: i32, out: *mut Ref) -> Status;
    fn CMBlockBufferCreateWithMemoryBlock(allocator: Ref, memory: *mut c_void, length: usize, block_allocator: Ref, custom: *const c_void, offset: usize, data_length: usize, flags: u32, out: *mut Ref) -> Status;
    fn CMSampleBufferCreateReady(allocator: Ref, data: Ref, format: Ref, samples: isize, timings: isize, timing: *const c_void, sizes_count: isize, sizes: *const usize, out: *mut Ref) -> Status;
    fn VTDecompressionSessionCreate(allocator: Ref, format: Ref, decoder_spec: Ref, image_attributes: Ref, callback: *const OutputCallbackRecord, out: *mut Ref) -> Status;
    fn VTDecompressionSessionDecodeFrame(session: Ref, sample: Ref, flags: u32, frame_ref: *mut c_void, info: *mut u32) -> Status;
    fn VTDecompressionSessionWaitForAsynchronousFrames(session: Ref) -> Status;
    fn VTDecompressionSessionInvalidate(session: Ref);
    fn CVPixelBufferLockBaseAddress(buffer: Ref, flags: u64) -> Status;
    fn CVPixelBufferUnlockBaseAddress(buffer: Ref, flags: u64) -> Status;
    fn CVPixelBufferGetBaseAddress(buffer: Ref) -> *const u8;
    fn CVPixelBufferGetBytesPerRow(buffer: Ref) -> usize;
    fn CVPixelBufferGetWidth(buffer: Ref) -> usize;
    fn CVPixelBufferGetHeight(buffer: Ref) -> usize;
}

const K_CF_NUMBER_SINT32: isize = 3;
const BGRA: i32 = 0x4247_5241;
const READ_ONLY: u64 = 1;

/// Where the session's callback leaves the frame `decode` waits on; boxed so its address holds.
type Slot = Option<Result<Frame, String>>;

pub struct Decoder {
    format: Ref,
    session: Ref,
    slot: Box<Slot>,
    sample: Vec<u8>,
}

// SAFETY: the session and format are used only from the thread that owns the decoder.
unsafe impl Send for Decoder {}

extern "C" fn output(refcon: *mut c_void, _: *mut c_void, status: Status, _: u32, image: Ref, _: CMTime, _: CMTime) {
    // SAFETY: `refcon` is the decoder's boxed slot, alive for the session's lifetime.
    let slot = unsafe { &mut *refcon.cast::<Slot>() };
    if status != 0 || image.is_null() {
        *slot = Some(Err(format!("VideoToolbox could not decode a frame ({status}).")));
        return;
    }
    // SAFETY: `image` is the BGRA CVPixelBuffer the session hands its callback; it is locked
    // read-only for the copy and every row read stays inside its bytes-per-row.
    unsafe {
        if CVPixelBufferLockBaseAddress(image, READ_ONLY) != 0 {
            return;
        }
        let (width, height, stride) = (CVPixelBufferGetWidth(image), CVPixelBufferGetHeight(image), CVPixelBufferGetBytesPerRow(image));
        let base = CVPixelBufferGetBaseAddress(image);
        if !base.is_null() && stride >= width * 4 {
            let mut rgba = Vec::with_capacity(width * height * 4);
            for y in 0..height {
                let row = std::slice::from_raw_parts(base.add(y * stride), width * 4);
                for px in row.chunks_exact(4) {
                    rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                }
            }
            *slot = Some(Ok(Frame { width: width as u32, height: height as u32, rgba }));
        }
        CVPixelBufferUnlockBaseAddress(image, READ_ONLY);
    }
}

impl Decoder {
    pub fn new(params: &Params) -> Result<Self, String> {
        // SAFETY: every pointer passed lives across its call; each object created here is
        // released once, by `Drop` or on the failure path below.
        unsafe {
            let pointers = [params.sps.as_ptr(), params.pps.as_ptr()];
            let sizes = [params.sps.len(), params.pps.len()];
            let mut format = null();
            let status = CMVideoFormatDescriptionCreateFromH264ParameterSets(null(), 2, pointers.as_ptr(), sizes.as_ptr(), 4, &mut format);
            if status != 0 || format.is_null() {
                return Err(format!("This device's H.264 stream isn't supported ({status})."));
            }
            let format_type = BGRA;
            let bgra = CFNumberCreate(null(), K_CF_NUMBER_SINT32, (&raw const format_type).cast());
            let keys = [kCVPixelBufferPixelFormatTypeKey];
            let values = [bgra];
            let attributes = CFDictionaryCreate(
                null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                (&raw const kCFTypeDictionaryKeyCallBacks).cast(),
                (&raw const kCFTypeDictionaryValueCallBacks).cast(),
            );
            CFRelease(bgra);
            let mut slot: Box<Slot> = Box::new(None);
            let record = OutputCallbackRecord { callback: output, refcon: (&raw mut *slot).cast() };
            let mut session = null();
            let status = VTDecompressionSessionCreate(null(), format, null(), attributes, &record, &mut session);
            CFRelease(attributes);
            if status != 0 || session.is_null() {
                CFRelease(format);
                return Err(format!("VideoToolbox could not start a decoder ({status})."));
            }
            Ok(Decoder { format, session, slot, sample: vec![] })
        }
    }

    pub fn decode(&mut self, nals: &[&[u8]]) -> Result<Option<Frame>, String> {
        self.sample.clear();
        // The format description carries the parameter sets; the sample is the picture.
        for nal in nals.iter().filter(|n| !matches!(n.first().map(|b| b & 0x1f), Some(7..=9))) {
            self.sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
            self.sample.extend_from_slice(nal);
        }
        if self.sample.is_empty() {
            return Ok(None);
        }
        *self.slot = None;
        // SAFETY: the block buffer borrows `self.sample` (kCFAllocatorNull never frees it) only
        // until the synchronous decode below returns; both buffers are released before return.
        unsafe {
            let mut block = null();
            let len = self.sample.len();
            if CMBlockBufferCreateWithMemoryBlock(null(), self.sample.as_mut_ptr().cast(), len, kCFAllocatorNull, null(), 0, len, 0, &mut block) != 0 {
                return Err("Could not wrap an H.264 sample.".into());
            }
            let mut sample = null();
            let status = CMSampleBufferCreateReady(null(), block, self.format, 1, 0, null(), 1, &len, &mut sample);
            CFRelease(block);
            if status != 0 {
                return Err("Could not wrap an H.264 sample.".into());
            }
            let status = VTDecompressionSessionDecodeFrame(self.session, sample, 0, null_mut(), null_mut());
            VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            CFRelease(sample);
            if status != 0 {
                return Err(format!("VideoToolbox could not decode a frame ({status})."));
            }
        }
        self.slot.take().transpose()
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: both were created in `new` and are released exactly once.
        unsafe {
            VTDecompressionSessionInvalidate(self.session);
            CFRelease(self.session);
            CFRelease(self.format);
        }
    }
}
