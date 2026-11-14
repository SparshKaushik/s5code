//! Web `QRCodeSvg` (level M): dark modules drawn as horizontal runs on white, with a quiet zone.
use qrcodegen::{QrCode, QrCodeEcc};
use rune::{El, el};

/// The symbol's width in modules, quiet zone included, and each row's dark runs as
/// `(x, y, width)` in modules.
fn runs(value: &str, margin: i32) -> Option<(i32, Vec<(i32, i32, i32)>)> {
    let qr = QrCode::encode_text(value, QrCodeEcc::Medium).ok()?;
    let n = qr.size();
    let mut out = Vec::new();
    for y in 0..n {
        let mut start = None;
        for x in 0..=n {
            if x < n && qr.get_module(x, y) {
                start.get_or_insert(x);
            } else if let Some(from) = start.take() {
                out.push((from + margin, y + margin, x - from));
            }
        }
    }
    Some((n + margin * 2, out))
}

/// At most `size` square, snapped to whole pixels per module so edges stay crisp.
pub fn view(value: &str, size: f32) -> El {
    let Some((modules, runs)) = runs(value, 2) else { return el().size(size, size) };
    let cell = (size / modules as f32).floor().max(1.0);
    let side = cell * modules as f32;
    el().size(side, side).fill(rune::Color::WHITE).role("img").label("Pairing link. Scan to open on another device").children(
        runs.into_iter().map(|(x, y, w)| el().absolute(x as f32 * cell, y as f32 * cell).size(w as f32 * cell, cell).fill(rune::Color::BLACK)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cover_the_finder_patterns_inside_the_quiet_zone() {
        let (modules, runs) = runs("HELLO", 2).unwrap();
        // Version 1 is 21 modules; two on each side for the quiet zone.
        assert_eq!(modules, 25);
        // The finder patterns' top edges: seven dark modules at each top corner.
        assert!(runs.contains(&(2, 2, 7)));
        assert!(runs.contains(&(16, 2, 7)));
        assert!(runs.iter().all(|&(x, y, w)| x >= 2 && y >= 2 && x + w <= 23 && y < 23));
        let long = format!("https://app.t3.codes/pair?host={}#token={}", "x".repeat(60), "y".repeat(40));
        assert!(runs_len(&long) > 25, "a longer link needs a larger version");
    }

    fn runs_len(value: &str) -> i32 {
        runs(value, 2).unwrap().0
    }
}
