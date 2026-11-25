//! Windows SnapShots, as Electron takes them: the foreground window (`windowsActiveWindow` over
//! `WindowsForeground.ts`) captured whole (`desktopCapturer`'s window source), here through
//! `PrintWindow`, which draws a window even where others overlap it.

use std::ffi::c_void;

use super::capture::Status;

/// The foreground window: its title, its program's file name, and its frame in physical pixels
/// and in the window's own DPI.
pub struct Front {
    pub title: String,
    pub owner: String,
    pub bounds: [i32; 4],
    pub dpi: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 1],
}

type Hwnd = isize;
type Handle = *mut c_void;

#[link(name = "user32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowTextW(hwnd: Hwnd, text: *mut u16, max: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, pid: *mut u32) -> u32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetDpiForWindow(hwnd: Hwnd) -> u32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn PrintWindow(hwnd: Hwnd, dc: Handle, flags: u32) -> i32;
    fn GetDC(hwnd: Hwnd) -> Handle;
    fn ReleaseDC(hwnd: Hwnd, dc: Handle) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(dc: Handle) -> Handle;
    fn CreateCompatibleBitmap(dc: Handle, width: i32, height: i32) -> Handle;
    fn SelectObject(dc: Handle, object: Handle) -> Handle;
    fn GetDIBits(dc: Handle, bitmap: Handle, start: u32, lines: u32, bits: *mut c_void, info: *mut BitmapInfo, usage: u32) -> i32;
    fn DeleteObject(object: Handle) -> i32;
    fn DeleteDC(dc: Handle) -> i32;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
    fn QueryFullProcessImageNameW(process: Handle, flags: u32, name: *mut u16, size: *mut u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
const PW_RENDERFULLCONTENT: u32 = 2;
const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

/// The window in front and the capture of it as RGBA, cropped to its visible frame (the DWM
/// bounds, without the invisible resize borders `GetWindowRect` includes).
pub fn capture() -> Result<(Front, image::RgbaImage), Status> {
    // SAFETY: plain Win32 calls on a window handle that may close at any time; each failure is
    // checked, and every DC and bitmap made here is released before returning.
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd == 0 || IsIconic(hwnd) != 0 {
            return Err(Status::Missing);
        }
        let mut outer = Rect::default();
        if GetWindowRect(hwnd, &mut outer) == 0 {
            return Err(Status::Missing);
        }
        let mut frame = Rect::default();
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, (&raw mut frame).cast(), size_of::<Rect>() as u32) < 0 {
            frame = outer;
        }
        let (width, height) = (outer.right - outer.left, outer.bottom - outer.top);
        if width <= 0 || height <= 0 {
            return Err(Status::Missing);
        }
        let screen = GetDC(0);
        let dc = CreateCompatibleDC(screen);
        let bitmap = CreateCompatibleBitmap(screen, width, height);
        let previous = SelectObject(dc, bitmap);
        let printed = PrintWindow(hwnd, dc, PW_RENDERFULLCONTENT) != 0;
        SelectObject(dc, previous);
        let mut bgra = vec![0u8; width as usize * height as usize * 4];
        let mut info = BitmapInfo {
            // Negative height: rows top-down.
            header: BitmapInfoHeader { size: size_of::<BitmapInfoHeader>() as u32, width, height: -height, planes: 1, bit_count: 32, compression: 0, size_image: 0, x_pels_per_meter: 0, y_pels_per_meter: 0, clr_used: 0, clr_important: 0 },
            colors: [0],
        };
        let read = printed && GetDIBits(dc, bitmap, 0, height as u32, bgra.as_mut_ptr().cast(), &mut info, 0) == height;
        DeleteObject(bitmap);
        DeleteDC(dc);
        ReleaseDC(0, screen);
        if !read {
            return Err(Status::Failed);
        }
        let crop = crop_box(&outer, &frame);
        let picture = rgba(&bgra, width as u32, height as u32, crop).ok_or(Status::Failed)?;
        let front = Front { title: window_text(hwnd), owner: owner(hwnd), bounds: [frame.left, frame.top, frame.right - frame.left, frame.bottom - frame.top], dpi: GetDpiForWindow(hwnd).max(96) };
        Ok((front, picture))
    }
}

/// The frame inside the window rectangle, as `(x, y, width, height)` from its corner.
fn crop_box(outer: &Rect, frame: &Rect) -> [u32; 4] {
    let left = (frame.left - outer.left).max(0);
    let top = (frame.top - outer.top).max(0);
    let right = frame.right.min(outer.right) - outer.left;
    let bottom = frame.bottom.min(outer.bottom) - outer.top;
    [left as u32, top as u32, (right - left).max(0) as u32, (bottom - top).max(0) as u32]
}

/// GDI's BGRA as opaque RGBA, cropped: `PrintWindow` leaves alpha undefined.
pub(crate) fn rgba(bgra: &[u8], width: u32, height: u32, [x, y, w, h]: [u32; 4]) -> Option<image::RgbaImage> {
    if w == 0 || h == 0 || x + w > width || y + h > height || bgra.len() < (width * height * 4) as usize {
        return None;
    }
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for row in y..y + h {
        let start = ((row * width + x) * 4) as usize;
        for p in bgra[start..start + (w * 4) as usize].chunks_exact(4) {
            out.extend([p[2], p[1], p[0], 255]);
        }
    }
    image::RgbaImage::from_raw(w, h, out)
}

unsafe fn window_text(hwnd: Hwnd) -> String {
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer's length bounds the write.
    let len = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    String::from_utf16_lossy(&buffer[..len.max(0) as usize])
}

/// The program's file name, as Electron's `path.split(/[\\/]/).pop()`.
unsafe fn owner(hwnd: Hwnd) -> String {
    let mut pid = 0;
    // SAFETY: each handle opened here is closed; the buffer's length bounds the write.
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return String::new();
        }
        let mut buffer = [0u16; 1024];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut size) != 0;
        CloseHandle(process);
        if !ok {
            return String::new();
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        path.rsplit(['\\', '/']).next().unwrap_or_default().to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_capture_crops_to_the_visible_frame_as_opaque_rgba() {
        let outer = Rect { left: 100, top: 50, right: 116, bottom: 60 };
        let frame = Rect { left: 107, top: 50, right: 109, bottom: 52 };
        let crop = crop_box(&outer, &frame);
        assert_eq!(crop, [7, 0, 2, 2], "the invisible resize borders are cut");
        let mut bgra = vec![0u8; 16 * 10 * 4];
        bgra[(7 * 4)..(7 * 4 + 4)].copy_from_slice(&[1, 2, 3, 0]);
        let picture = rgba(&bgra, 16, 10, crop).unwrap();
        assert_eq!(picture.dimensions(), (2, 2));
        assert_eq!(picture.get_pixel(0, 0).0, [3, 2, 1, 255], "BGRA to RGBA, alpha made opaque");
        assert!(rgba(&bgra, 16, 10, [15, 0, 2, 2]).is_none(), "a crop past the edge");
        assert_eq!(crop_box(&outer, &Rect { left: 90, top: 40, right: 200, bottom: 70 }), [0, 0, 16, 10], "a frame larger than the window is clamped");
    }
}
