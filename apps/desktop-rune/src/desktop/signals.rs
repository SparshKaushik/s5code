//! Electron's DesktopLifecycle signal handling: SIGINT and SIGTERM quit the way the menu does, so
//! the bundled server and SSH servers stop instead of outliving the app. A shutdown that stalls
//! exits after Electron's grace period anyway.

use std::sync::atomic::{AtomicI32, Ordering};
use std::time::Duration;

const SIGINT: i32 = 2;
const SIGTERM: i32 = 15;
const STALLED_SHUTDOWN: Duration = Duration::from_secs(10);

static RECEIVED: AtomicI32 = AtomicI32::new(0);

unsafe extern "C" {
    fn signal(signum: i32, handler: extern "C" fn(i32)) -> usize;
}

/// Only an atomic store: anything else is unsafe inside a signal handler.
extern "C" fn on_signal(signum: i32) {
    RECEIVED.store(signum, Ordering::SeqCst);
}

pub fn install(app: rune::AppHandle<crate::App>) {
    if rune::is_headless() {
        return;
    }
    unsafe {
        signal(SIGINT, on_signal);
        signal(SIGTERM, on_signal);
    }
    std::thread::spawn(move || {
        let signum = loop {
            std::thread::sleep(Duration::from_millis(100));
            let signum = RECEIVED.load(Ordering::SeqCst);
            if signum != 0 {
                break signum;
            }
        };
        app.update(|_, tx| tx.quit());
        std::thread::sleep(STALLED_SHUTDOWN);
        eprintln!("s5code-rune: shutdown stalled after signal {signum}; exiting");
        crate::host::shutdown();
        std::process::exit(128 + signum);
    });
}
