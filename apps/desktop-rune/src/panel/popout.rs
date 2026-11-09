//! Electron's separate preview window: an always-on-top, view-only window that mirrors a
//! browser tab's frames, 480 × 320 to start and never under 240 × 160.
use std::cell::RefCell;

use rune::{AppHandle, AppSpec, Cx, El, Pixels, col, el};

#[derive(Default)]
pub struct Popout {
    pixels: Option<Pixels>,
}

thread_local! {
    static HANDLE: RefCell<Option<AppHandle<Popout>>> = const { RefCell::new(None) };
}

/// Adds the window, hidden until `open`, to the app's event loop.
pub fn register(app: &mut rune::App<crate::App>) {
    let spec = AppSpec { min_size: Some((240., 160.)), ..AppSpec::new("Browser preview", (480., 320.), Popout::default, view) };
    let options = rune::WindowOptions { always_on_top: true, ..Default::default() };
    let window = rune::App::new(spec).windowless().window_options(options);
    let (handle, _) = app.window(rune::WindowSpec::from_app("browser-preview", window));
    HANDLE.with(|h| *h.borrow_mut() = Some(handle));
}

fn with(f: impl FnOnce(&AppHandle<Popout>)) {
    HANDLE.with(|h| {
        if let Some(handle) = h.borrow().as_ref() {
            f(handle);
        }
    });
}

pub fn visible() -> bool {
    HANDLE.with(|h| h.borrow().as_ref().is_some_and(AppHandle::is_visible))
}

pub fn open(pixels: Pixels) {
    with(|h| {
        h.update(move |s, _| s.pixels = Some(pixels));
        h.open();
    });
}

pub fn retitle(title: String) {
    with(|h| h.update(move |_, tx| tx.set_title(title)));
}

pub fn close() {
    with(|h| h.close());
}

fn view(s: &Popout, cx: &mut Cx<Popout>) -> El {
    let t = crate::theme::theme(cx.system_dark());
    let root = col().w_full().h_full().fill(t.c.background);
    match &s.pixels {
        Some(p) => root.child(el().w_full().h_full().pixels_fit(p, rune::SurfaceFit::Fit).role("img").label("Live browser preview")),
        None => root,
    }
}
