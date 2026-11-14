//! What Rune's menu API can't express of DesktopApplicationMenu.ts, and the window frame
//! autosave. Raw objc calls as in notify.rs, so the lockfile CI builds with stays unchanged.
//! Every lookup that misses leaves the menu as it was.

use std::ffi::{CStr, CString, c_char, c_void};

type Id = *mut c_void;

#[link(name = "objc")]
unsafe extern "C" {
    fn objc_getClass(name: *const c_char) -> Id;
    fn sel_registerName(name: *const c_char) -> Id;
    fn objc_msgSend();
}

/// `objc_msgSend` cast to the method's exact signature.
macro_rules! msg {
    ($recv:expr, $sel:expr $(, $arg:expr => $ty:ty)* ; $ret:ty) => {{
        let f: unsafe extern "C" fn(Id, Id $(, $ty)*) -> $ret = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f($recv, sel($sel) $(, $arg)*)
    }};
}

const SHIFT: usize = 1 << 17;
const CONTROL: usize = 1 << 18;
const COMMAND: usize = 1 << 20;

fn sel(name: &CStr) -> Id {
    // SAFETY: registers or looks up a selector; any C string is valid.
    unsafe { sel_registerName(name.as_ptr()) }
}

fn class(name: &CStr) -> Id {
    // SAFETY: looks up a class by name; null when absent.
    unsafe { objc_getClass(name.as_ptr()) }
}

// SAFETY (for every function below): main thread only (on_window runs there); each call names a
// documented AppKit/Foundation selector with its exact argument and return types, on objects
// AppKit keeps alive. Items are retained across a move so removing them never frees them.

unsafe fn ns_string(s: &str) -> Id {
    let c = CString::new(s).unwrap_or_default();
    unsafe { msg!(class(c"NSString"), c"stringWithUTF8String:", c.as_ptr() => *const c_char; Id) }
}

unsafe fn rust_string(ns: Id) -> String {
    if ns.is_null() {
        return String::new();
    }
    unsafe {
        let p = msg!(ns, c"UTF8String"; *const c_char);
        if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

unsafe fn items(menu: Id) -> Vec<Id> {
    unsafe {
        let n = msg!(menu, c"numberOfItems"; isize);
        (0..n).map(|i| msg!(menu, c"itemAtIndex:", i => isize; Id)).filter(|i| !i.is_null()).collect()
    }
}

unsafe fn title(item: Id) -> String {
    unsafe { rust_string(msg!(item, c"title"; Id)) }
}

unsafe fn action(item: Id) -> Id {
    unsafe { msg!(item, c"action"; Id) }
}

unsafe fn submenu(item: Id) -> Id {
    unsafe { msg!(item, c"submenu"; Id) }
}

unsafe fn find(menu: Id, f: impl Fn(Id) -> bool) -> Option<Id> {
    unsafe { items(menu).into_iter().find(|&i| f(i)) }
}

unsafe fn find_action(menu: Id, name: &CStr) -> Option<Id> {
    let wanted = sel(name);
    unsafe { find(menu, |i| action(i) == wanted) }
}

unsafe fn top_menu(root: Id, name: &str) -> Option<Id> {
    unsafe { find(root, |i| title(i) == name).map(|i| submenu(i)).filter(|m| !m.is_null()) }
}

unsafe fn index(menu: Id, item: Id) -> isize {
    unsafe { msg!(menu, c"indexOfItem:", item => Id; isize) }
}

unsafe fn remove(menu: Id, item: Id) {
    unsafe { msg!(menu, c"removeItem:", item => Id; ()) }
}

unsafe fn insert(menu: Id, item: Id, at: isize) {
    unsafe {
        let count = msg!(menu, c"numberOfItems"; isize);
        msg!(menu, c"insertItem:atIndex:", item => Id, at.clamp(0, count) => isize; ())
    }
}

unsafe fn add(menu: Id, item: Id) {
    unsafe { msg!(menu, c"addItem:", item => Id; ()) }
}

/// Moves `item` within `menu` to `at` (an index in the menu without it), or to the end.
unsafe fn move_item(menu: Id, item: Id, at: Option<isize>) {
    unsafe {
        msg!(item, c"retain"; Id);
        remove(menu, item);
        match at {
            Some(at) => insert(menu, item, at),
            None => add(menu, item),
        }
        msg!(item, c"release"; ());
    }
}

unsafe fn separator() -> Id {
    unsafe { msg!(class(c"NSMenuItem"), c"separatorItem"; Id) }
}

/// A nil-target item: AppKit sends `action` to the first responder (the window for zoom and
/// full screen, Rune's view for paste).
unsafe fn new_item(title: &str, action: &CStr, key: &str, mask: usize) -> Id {
    unsafe {
        let alloc = msg!(class(c"NSMenuItem"), c"alloc"; Id);
        let item = msg!(alloc, c"initWithTitle:action:keyEquivalent:", ns_string(title) => Id, sel(action) => Id, ns_string(key) => Id; Id);
        msg!(item, c"setKeyEquivalentModifierMask:", mask => usize; ());
        item
    }
}

unsafe fn set_key(item: Id, key: &str, mask: usize) {
    unsafe {
        msg!(item, c"setKeyEquivalent:", ns_string(key) => Id; ());
        msg!(item, c"setKeyEquivalentModifierMask:", mask => usize; ());
    }
}

/// Reshapes the menu bar Rune and winit built into Electron's: the app menu (About, Settings,
/// Services, Hide, Quit), Edit › Paste as Text, View › Toggle Full Screen, Window › Minimize,
/// Zoom, Bring All to Front with no Close, and File, Edit, View, Window in that order.
pub fn fix_menus(settings_title: &str, updates_title: &str, quit_title: &str) {
    unsafe {
        let app = msg!(class(c"NSApplication"), c"sharedApplication"; Id);
        if app.is_null() {
            return;
        }
        let root = msg!(app, c"mainMenu"; Id);
        if root.is_null() {
            return;
        }
        if let Some(app_menu) = items(root).first().map(|&i| submenu(i)).filter(|m| !m.is_null()) {
            fix_app_menu(app_menu, settings_title, updates_title, quit_title);
        }
        if let Some(edit) = top_menu(root, "Edit")
            && let Some(paste) = find_action(edit, c"paste:")
            && find(edit, |i| title(i) == "Paste as Text").is_none()
        {
            insert(edit, new_item("Paste as Text", c"paste:", "v", COMMAND | SHIFT), index(edit, paste) + 1);
        }
        if let Some(view) = top_menu(root, "View")
            && find_action(view, c"toggleFullScreen:").is_none()
        {
            add(view, separator());
            add(view, new_item("Toggle Full Screen", c"toggleFullScreen:", "f", COMMAND | CONTROL));
        }
        if let Some(window) = top_menu(root, "Window") {
            // Cmd+W belongs to the app's tab and terminal close; it never closes the window.
            if let Some(close) = find_action(window, c"performClose:") {
                remove(window, close);
            }
            if find_action(window, c"performZoom:").is_none() {
                add(window, new_item("Zoom", c"performZoom:", "", 0));
                add(window, separator());
                add(window, new_item("Bring All to Front", c"arrangeInFront:", "", 0));
            }
            msg!(app, c"setWindowsMenu:", window => Id; ());
        }
        for (k, name) in ["File", "Edit", "View", "Window"].into_iter().enumerate() {
            if let Some(top) = find(root, |i| title(i) == name) {
                move_item(root, top, Some(1 + k as isize));
            }
        }
    }
}

unsafe fn fix_app_menu(menu: Id, settings_title: &str, updates_title: &str, quit_title: &str) {
    unsafe {
        // winit's Quit terminates at once; the app's own Quit lets Cmd+Q reach the quit hold.
        if let Some(terminate) = find_action(menu, c"terminate:") {
            remove(menu, terminate);
        }
        if let Some(quit) = find(menu, |i| title(i) == quit_title) {
            move_item(menu, quit, None);
            // AppKit drops an equivalent added while another item still held it.
            set_key(quit, "q", COMMAND);
        }
        if let (Some(settings), Some(about)) = (find(menu, |i| title(i) == settings_title), find_action(menu, c"orderFrontStandardAboutPanel:")) {
            msg!(settings, c"retain"; Id);
            remove(menu, settings);
            let at = index(menu, about) + 2;
            insert(menu, settings, at);
            insert(menu, separator(), at + 1);
            msg!(settings, c"release"; ());
            set_key(settings, ",", COMMAND);
        }
        if let (Some(updates), Some(about)) = (find(menu, |i| title(i) == updates_title), find_action(menu, c"orderFrontStandardAboutPanel:")) {
            msg!(updates, c"retain"; Id);
            remove(menu, updates);
            insert(menu, updates, index(menu, about) + 1);
            msg!(updates, c"release"; ());
        }
        // winit titles About, Hide and Quit with the executable's name.
        let info = msg!(class(c"NSProcessInfo"), c"processInfo"; Id);
        let process = rust_string(msg!(info, c"processName"; Id));
        if !process.is_empty() && process != crate::desktop::APP_NAME {
            for item in items(menu) {
                let old = title(item);
                if old.contains(&process) {
                    msg!(item, c"setTitle:", ns_string(&old.replace(&process, crate::desktop::APP_NAME)) => Id; ());
                }
            }
        }
    }
}

/// Restores the window's last frame and keeps saving it (AppKit's frame autosave, which also
/// covers the zoomed state), as Electron's persisted main-window bounds.
pub fn autosave_frame(window: &rune::WindowHandle) {
    let _ = window.with_raw_handle(|handle| {
        let rune::raw_window_handle::RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        unsafe {
            let ns_window = msg!(h.ns_view.as_ptr(), c"window"; Id);
            if ns_window.is_null() {
                return;
            }
            let name = ns_string("S5CodeMainWindow");
            msg!(ns_window, c"setFrameUsingName:", name => Id; i8);
            msg!(ns_window, c"setFrameAutosaveName:", name => Id; i8);
        }
    });
}

/// Electron's `showMessageBox` info box. Runs on the next run loop pass, outside the event
/// handler that asked for it, so its modal loop never re-enters one.
pub fn alert(message: &str, detail: &str) {
    unsafe {
        let alloc = msg!(class(c"NSAlert"), c"alloc"; Id);
        let alert = msg!(alloc, c"init"; Id);
        if alert.is_null() {
            return;
        }
        msg!(alert, c"setMessageText:", ns_string(message) => Id; ());
        msg!(alert, c"setInformativeText:", ns_string(detail) => Id; ());
        // The pending perform retains the alert until it has run.
        msg!(alert, c"performSelectorOnMainThread:withObject:waitUntilDone:", sel(c"runModal") => Id, std::ptr::null_mut::<c_void>() => Id, 0i8 => i8; ());
        msg!(alert, c"release"; ());
    }
}

/// Electron's `nativeTheme.themeSource`: the app's appearance, so AppKit's title bar, menus,
/// alerts and file dialogs match the theme; `None` follows the system.
pub fn set_appearance(dark: Option<bool>) {
    unsafe {
        let app = msg!(class(c"NSApplication"), c"sharedApplication"; Id);
        if app.is_null() {
            return;
        }
        let appearance = match dark {
            None => std::ptr::null_mut(),
            Some(dark) => {
                let name = ns_string(if dark { "NSAppearanceNameDarkAqua" } else { "NSAppearanceNameAqua" });
                msg!(class(c"NSAppearance"), c"appearanceNamed:", name => Id; Id)
            }
        };
        msg!(app, c"setAppearance:", appearance => Id; ());
    }
}

/// `[NSLocale currentLocale].localeIdentifier`, as Electron's `getSystemLocale` reads the region.
pub fn locale_identifier() -> Option<String> {
    unsafe {
        let locale = msg!(class(c"NSLocale"), c"currentLocale"; Id);
        if locale.is_null() {
            return None;
        }
        let id = rust_string(msg!(locale, c"localeIdentifier"; Id));
        (!id.is_empty()).then_some(id)
    }
}
