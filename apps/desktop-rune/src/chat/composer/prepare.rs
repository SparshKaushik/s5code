//! prepareImageForAttachment: a queued picture over 2000 pixels on a side, or over the 10 MiB an
//! image sends as, is fitted and re-encoded off the UI thread before it can be sent; one that
//! can't be read or brought under the limit is refused with web's reason. Its tile keeps the
//! original's thumbnail meanwhile.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use image::{DynamicImage, GenericImageView, imageops::FilterType};

/// IMAGE_ATTACHMENT_MAX_EDGE.
const MAX_EDGE: u32 = 2000;
/// MAX_COMPRESSIBLE_SOURCE_BYTES.
const MAX_SOURCE: usize = 50 * 1024 * 1024;
const QUALITY: [u8; 4] = [92, 85, 78, 68];
const SCALES: [f32; 3] = [1.0, 0.75, 0.55];

#[derive(Debug, PartialEq)]
pub enum Failure {
    TooLarge,
    Unreadable,
}

/// Web's thread error for a refused picture.
pub fn message(name: &str, failure: &Failure) -> String {
    match failure {
        Failure::Unreadable => format!("'{name}' could not be read as an image."),
        Failure::TooLarge => format!("'{name}' is too large to attach, even after compression."),
    }
}

fn downscalable(mime: &str) -> bool {
    matches!(mime, "image/png" | "image/jpeg")
}

/// Whether the queued picture at `path` must be prepared, from its size and header.
pub fn needed(path: &str, mime: &str, max: u64) -> bool {
    let size = std::fs::metadata(path).map_or(0, |m| m.len());
    size > max || (downscalable(mime) && image::image_dimensions(path).is_ok_and(|(w, h)| w.max(h) > MAX_EDGE))
}

fn encode(image: &DynamicImage, mime: &str, quality: u8) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if mime == "image/png" {
        image.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    } else {
        // JPEG has no alpha: a white matte, as web's canvas.
        let rgba = image.to_rgba8();
        let rgb = image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
            let [r, g, b, a] = rgba.get_pixel(x, y).0;
            let over = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
            image::Rgb([over(r), over(g), over(b)])
        });
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(&rgb).ok()?;
    }
    Some(out)
}

fn resized(image: &DynamicImage, longest: u32) -> DynamicImage {
    let (w, h) = image.dimensions();
    let scale = (longest as f32 / w.max(h) as f32).min(1.0);
    if scale >= 1.0 {
        return image.clone();
    }
    let size = |v: u32| ((v as f32 * scale).round() as u32).clamp(1, longest);
    image.resize_exact(size(w), size(h), FilterType::Triangle)
}

/// compressImageToByteLimit: the bytes and type to send instead, or `None` to send the file as
/// it is. `max` is the byte limit.
pub fn prepare(bytes: &[u8], mime: &str, max: usize) -> Result<Option<(Vec<u8>, &'static str)>, Failure> {
    let mime: &'static str = if mime == "image/png" { "image/png" } else if mime == "image/jpeg" { "image/jpeg" } else { "" };
    let decoded = (bytes.len() <= MAX_SOURCE).then(|| image::load_from_memory(bytes).ok()).flatten();
    // fitImageToMaxEdge: a PNG or JPEG over the edge, at the edge in its own type.
    let fitted = decoded
        .as_ref()
        .filter(|i| !mime.is_empty() && i.width().max(i.height()) > MAX_EDGE)
        .map(|i| resized(i, MAX_EDGE))
        .and_then(|i| Some((encode(&i, mime, QUALITY[0])?, i)));
    match &fitted {
        Some((data, _)) if data.len() <= max => return Ok(Some((data.clone(), mime))),
        None if bytes.len() <= max => return Ok(None),
        _ => {}
    }
    if bytes.len() > MAX_SOURCE {
        return Err(Failure::TooLarge);
    }
    // reencodeWithinBudget: JPEG at falling quality, then smaller, until its data URL fits.
    let image = match fitted {
        Some((_, image)) => image,
        None => decoded.ok_or(Failure::Unreadable)?,
    };
    let budget = max / 3 * 4;
    let base = MAX_EDGE.min(image.width().max(image.height()));
    for scale in SCALES {
        let target = resized(&image, ((base as f32 * scale).round() as u32).max(1));
        for quality in QUALITY {
            let data = encode(&target, "image/jpeg", quality).ok_or(Failure::Unreadable)?;
            if data.len().div_ceil(3) * 4 <= budget {
                return Ok(Some((data, "image/jpeg")));
            }
        }
    }
    Err(Failure::TooLarge)
}

/// fileNameForMimeType: `name` with the extension its new type takes.
pub fn renamed(name: &str, mime: &str) -> String {
    let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    let stem = if stem.is_empty() { "image" } else { stem };
    format!("{stem}.{}", if mime == "image/png" { "png" } else { "jpg" })
}

pub type Slot = Arc<Mutex<Option<Result<Option<PathBuf>, Failure>>>>;

/// Prepares the picture at `path` on a thread of its own; the result lands in the slot, a
/// replacement file in a folder of its own so its name is kept.
pub fn start(path: String, name: String, mime: &'static str, max: usize, waker: Option<rune::Waker>) -> Slot {
    let slot = Slot::default();
    let into = slot.clone();
    std::thread::spawn(move || {
        let result = std::fs::read(&path).map_err(|_| Failure::Unreadable).and_then(|bytes| prepare(&bytes, mime, max)).and_then(|prepared| {
            let Some((data, mime)) = prepared else { return Ok(None) };
            let dir = std::env::temp_dir().join(format!("s5-prepared-{}", s5_net::uuid()));
            let out = dir.join(renamed(&name, mime));
            std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&out, data)).map_err(|_| Failure::Unreadable)?;
            // A snapshot keeps its source beside the new picture.
            let source = crate::desktop::capture::source_path(&path);
            if std::path::Path::new(&source).exists() {
                let _ = std::fs::copy(&source, crate::desktop::capture::source_path(&out.display().to_string()));
            }
            Ok(Some(out))
        });
        *into.lock().unwrap() = Some(result);
        if let Some(wake) = waker {
            wake();
        }
    });
    slot
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32, noise: bool) -> Vec<u8> {
        let mut seed = 7u32;
        let image = image::RgbaImage::from_fn(w, h, |x, y| {
            if noise {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let [a, b, c, _] = seed.to_le_bytes();
                image::Rgba([a, b, c, 255])
            } else {
                image::Rgba([(x % 256) as u8, (y % 256) as u8, 90, 255])
            }
        });
        let mut out = Vec::new();
        DynamicImage::ImageRgba8(image).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn pictures_fit_the_edge_then_the_byte_limit_or_are_refused() {
        // Small enough already: sent as it is.
        assert_eq!(prepare(&png(40, 20, false), "image/png", 10 * 1024 * 1024), Ok(None));
        // Over the edge: fitted, keeping its type.
        let (data, mime) = prepare(&png(3000, 1000, false), "image/png", 10 * 1024 * 1024).unwrap().unwrap();
        assert_eq!((mime, image::load_from_memory(&data).unwrap().dimensions()), ("image/png", (2000, 667)));
        // Over the byte limit: re-encoded as JPEG within it.
        let (data, mime) = prepare(&png(300, 300, true), "image/png", 64 * 1024).unwrap().unwrap();
        assert!(mime == "image/jpeg" && data.len().div_ceil(3) * 4 <= 64 * 1024 / 3 * 4);
        // Nothing gets noise under a few hundred bytes; bytes that aren't a picture can't be read.
        assert_eq!(prepare(&png(300, 300, true), "image/png", 300), Err(Failure::TooLarge));
        assert_eq!(prepare(&[7u8; 4096], "image/png", 1024), Err(Failure::Unreadable));
        assert_eq!(renamed("shot.png", "image/jpeg"), "shot.jpg");
    }
}
