use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, EventControllerKey};
use gtk4::gio;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Once;

use crate::widgets;

static GTK_INIT: Once = Once::new();

/// Ensure GTK is initialized (safe to call multiple times).
pub(crate) fn ensure_gtk_init() {
    GTK_INIT.call_once(|| {
        gtk4::init().expect("Failed to initialize GTK4");
    });
}

thread_local! {
    static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    /// Buffered keyboard shortcuts registered before the app is running.
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = RefCell::new(Vec::new());
    /// Callback map for keyboard shortcuts.
    static SHORTCUT_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    /// Counter for generating unique callback keys.
    static NEXT_CALLBACK_KEY: RefCell<usize> = RefCell::new(1);
    /// Reference to the GTK Application (public for sheet/toolbar/window/system access).
    pub(crate) static GTK_APP: RefCell<Option<Application>> = RefCell::new(None);
    /// Timer callbacks (interval_ms, callback f64)
    static TIMER_CALLBACKS: RefCell<Vec<(f64, f64)>> = RefCell::new(Vec::new());
    /// App lifecycle callbacks
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
}

struct PendingShortcut {
    key_ptr: *const u8,
    modifiers: f64,
    callback: f64,
}

// SAFETY: PendingShortcut contains a raw pointer that is only dereferenced on the main thread
// where it was created. We need Send to store it in a thread_local RefCell.
unsafe impl Send for PendingShortcut {}

struct AppEntry {
    title: String,
    width: f64,
    height: f64,
    root_handle: Option<i64>,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
}

/// Extract a &str from a *const StringHeader pointer.
pub(crate) fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).length as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create an app with title, width, height.
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    ensure_gtk_init();

    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        apps.push(AppEntry {
            title,
            width: w,
            height: h,
            root_handle: None,
            min_size: None,
            max_size: None,
        });
        apps.len() as i64 // 1-based handle
    })
}

/// Set the root widget (body) of the app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_handle = Some(root_handle);
        }
    });
}

/// Run the application event loop (blocks).
pub fn app_run(_app_handle: i64) {
    let app = Application::builder()
        .application_id("com.perry.app")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    GTK_APP.with(|ga| {
        *ga.borrow_mut() = Some(app.clone());
    });

    app.connect_activate(move |app| {
        APPS.with(|a| {
            let apps = a.borrow();
            for entry in apps.iter() {
                let window = ApplicationWindow::builder()
                    .application(app)
                    .title(&entry.title)
                    .default_width(entry.width as i32)
                    .default_height(entry.height as i32)
                    .build();

                // Set min/max size hints via GDK geometry
                if let Some((min_w, min_h)) = entry.min_size {
                    window.set_size_request(min_w as i32, min_h as i32);
                }

                // Note: GTK4 doesn't have a direct max-size API. Apps typically
                // use set_resizable(false) or handle size constraints differently.
                // We store it but GTK4 relies on window manager for max size.

                if let Some(root_handle) = entry.root_handle {
                    if let Some(widget) = widgets::get_widget(root_handle) {
                        window.set_child(Some(&widget));
                    }
                }

                // Install keyboard shortcuts on this window
                install_shortcuts_on_window(&window);

                window.present();

                // Install pending menu bar (show_menubar must be set per window)
                crate::menu::PENDING_MENUBAR.with(|p| {
                    if p.borrow().is_some() {
                        window.set_show_menubar(true);
                    }
                });
            }
        });

        // Install pending menu bar on the application
        crate::menu::PENDING_MENUBAR.with(|p| {
            if let Some(bar_handle) = *p.borrow() {
                crate::menu::install_menubar_on_app(app, bar_handle);
            }
        });

        // Install timers
        TIMER_CALLBACKS.with(|tc| {
            for (interval_ms, callback) in tc.borrow().iter() {
                let cb = *callback;
                let ms = *interval_ms as u64;
                glib::timeout_add_local(std::time::Duration::from_millis(ms), move || {
                    // Drain resolved promises, then run microtasks (.then callbacks)
                    unsafe {
                        js_stdlib_process_pending();
                        js_promise_run_microtasks();
                    }
                    let ptr = unsafe { js_nanbox_get_pointer(cb) } as *const u8;
                    unsafe { js_closure_call0(ptr); }
                    glib::ControlFlow::Continue
                });
            }
        });

        // Call on_activate callback
        ON_ACTIVATE_CALLBACK.with(|cb| {
            if let Some(callback) = *cb.borrow() {
                let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                unsafe { js_closure_call0(ptr); }
            }
        });
    });

    // Install shutdown handler for on_terminate
    app.connect_shutdown(move |_app| {
        ON_TERMINATE_CALLBACK.with(|cb| {
            if let Some(callback) = *cb.borrow() {
                let ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
                unsafe { js_closure_call0(ptr); }
            }
        });
    });

    // GTK Application::run() blocks like NSApplication.run()
    // Pass empty args since we handle our own argument parsing
    let empty: Vec<String> = vec![];
    app.run_with_args(&empty);
}

/// Set the minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].min_size = Some((w, h));
        }
    });
}

/// Set the maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].max_size = Some((w, h));
        }
    });
}

/// Install keyboard shortcuts on a window using EventControllerKey.
fn install_shortcuts_on_window(window: &ApplicationWindow) {
    let controller = EventControllerKey::new();

    controller.connect_key_pressed(move |_controller, keyval, _keycode, modifier| {
        let key_name = keyval.name().map(|n| n.to_string()).unwrap_or_default();

        // Find matching shortcut
        let matched = PENDING_SHORTCUTS.with(|ps| {
            let shortcuts = ps.borrow();
            for shortcut in shortcuts.iter() {
                let shortcut_key = str_from_header(shortcut.key_ptr);

                // Convert Perry modifier bits to GDK modifier state
                let mod_bits = shortcut.modifiers as u64;
                // Perry: 1=Cmd, 2=Shift, 4=Option, 8=Control
                // On Linux: Cmd maps to Ctrl
                let mut expected = gdk::ModifierType::empty();
                if mod_bits & 1 != 0 { expected |= gdk::ModifierType::CONTROL_MASK; } // Cmd -> Ctrl
                if mod_bits & 2 != 0 { expected |= gdk::ModifierType::SHIFT_MASK; }
                if mod_bits & 4 != 0 { expected |= gdk::ModifierType::ALT_MASK; } // Option -> Alt
                if mod_bits & 8 != 0 { expected |= gdk::ModifierType::CONTROL_MASK; }

                // Check key match (case-insensitive single char)
                let key_matches = key_name.eq_ignore_ascii_case(shortcut_key);

                // Check modifier match (mask out irrelevant bits)
                let relevant = gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::SHIFT_MASK
                    | gdk::ModifierType::ALT_MASK;
                let mod_matches = (modifier & relevant) == (expected & relevant);

                if key_matches && mod_matches {
                    return Some(shortcut.callback);
                }
            }
            None
        });

        if let Some(callback) = matched {
            let closure_ptr = unsafe { js_nanbox_get_pointer(callback) };
            unsafe {
                js_closure_call0(closure_ptr as *const u8);
            }
            glib::Propagation::Stop
        } else {
            glib::Propagation::Proceed
        }
    });

    window.add_controller(controller);
}

/// Add a keyboard shortcut.
/// `key_ptr` is a StringHeader pointer to the key character (e.g., "s" for Cmd+S).
/// `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
/// `callback` is a NaN-boxed closure pointer.
///
/// On Linux, Cmd (modifier 1) is transparently remapped to Ctrl.
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    PENDING_SHORTCUTS.with(|ps| {
        ps.borrow_mut().push(PendingShortcut { key_ptr, modifiers, callback });
    });
}

/// Set a repeating timer. interval_ms = milliseconds between ticks.
pub fn set_timer(interval_ms: f64, callback: f64) {
    TIMER_CALLBACKS.with(|tc| {
        tc.borrow_mut().push((interval_ms, callback));
    });
}

/// Register an on_activate callback (called when the app becomes active).
pub fn on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Register an on_terminate callback (called when the app is shutting down).
pub fn on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}
