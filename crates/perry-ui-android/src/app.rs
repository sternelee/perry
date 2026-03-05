use jni::objects::JValue;
use std::cell::RefCell;

use crate::jni_bridge;
use crate::widgets;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

thread_local! {
    static PENDING_CONFIG: RefCell<Option<AppConfig>> = RefCell::new(None);
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
}

struct AppConfig {
    _title: String,
    _width: f64,
    _height: f64,
}

/// Extract a &str from a *const StringHeader pointer (Perry runtime string format).
pub fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Create an app. Stores config for deferred creation. Returns app handle (i64).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    PENDING_CONFIG.with(|c| {
        *c.borrow_mut() = Some(AppConfig {
            _title: title,
            _width: w,
            _height: h,
        });
    });

    1 // Single app handle
}

/// Global body handle — set from any thread, read from any thread.
/// Workaround: Perry's App() intrinsic extracts the body handle using
/// js_nanbox_get_pointer, which returns 0 for NaN-boxed integers (widget handles
/// are small integers, not heap pointers). We store the last widget added via
/// widgetAddChild as a fallback.
static GLOBAL_BODY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// Set the root widget (body) of the app.
pub fn app_set_body(_app_handle: i64, root_handle: i64) {
    unsafe {
        __android_log_print(
            3, b"PerryApp\0".as_ptr(),
            b"app_set_body: root_handle=%lld\0".as_ptr(),
            root_handle,
        );
    }
    if root_handle > 0 {
        GLOBAL_BODY.store(root_handle, std::sync::atomic::Ordering::Relaxed);
    }
    PENDING_BODY.with(|b| {
        *b.borrow_mut() = Some(root_handle);
    });
}

/// Called from widgetClearChildren to track which handle is the root.
/// The first widget that has clearChildren called on it is likely the root.
pub fn track_root_candidate(handle: i64) {
    // Only set if not already set by app_set_body
    let current = GLOBAL_BODY.load(std::sync::atomic::Ordering::Relaxed);
    if current == 0 {
        GLOBAL_BODY.store(handle, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Attach the root widget to the Activity's content view.
/// Called from the native init thread after all widgets are built.
/// Posts to the UI thread to add the root view to the FrameLayout.
fn attach_root_to_activity() {
    unsafe {
        __android_log_print(
            3, b"PerryApp\0".as_ptr(),
            b"attach_root_to_activity: called\0".as_ptr(),
        );
    }
    // Get root handle: prefer PENDING_BODY, fall back to GLOBAL_BODY
    let mut root_handle = PENDING_BODY.with(|b| {
        b.borrow().unwrap_or(0)
    });
    // If body handle is invalid (0), use the global fallback
    if root_handle <= 0 {
        root_handle = GLOBAL_BODY.load(std::sync::atomic::Ordering::Relaxed);
    }
    unsafe {
        __android_log_print(
            3, b"PerryApp\0".as_ptr(),
            b"attach_root_to_activity: final root_handle=%lld\0".as_ptr(),
            root_handle,
        );
    }
    if root_handle > 0 {
        if let Some(root_ref) = widgets::get_widget(root_handle) {
            let mut env = jni_bridge::get_env();
            let root_obj = root_ref.as_obj();
            let bridge_class = jni_bridge::with_cache(|c| {
                env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
            });
            let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
            let _ = env.call_static_method(
                bridge_cls,
                "setContentView",
                "(Landroid/view/View;)V",
                &[JValue::Object(root_obj)],
            );
            unsafe {
                __android_log_print(
                    3, b"PerryApp\0".as_ptr(),
                    b"attach_root_to_activity: setContentView called\0".as_ptr(),
                );
            }
        }
    }
}

/// Run the app event loop.
/// On Android, the event loop is the Activity lifecycle managed by the system.
/// This just attaches the root widget to the Activity and returns.
/// Unlike macOS/iOS, this does NOT block — the Activity keeps running.
pub fn app_run(_app_handle: i64) {
    unsafe {
        __android_log_print(
            3, b"PerryApp\0".as_ptr(),
            b"app_run: called\0".as_ptr(),
        );
    }
    // Attach the root widget to the Activity
    attach_root_to_activity();

    // On Android we run on the UI thread, so we must NOT block.
    // The Activity lifecycle IS the event loop.
}

/// Called when the Activity is destroyed. No-op since App() doesn't block on Android.
pub fn signal_shutdown() {
    // Nothing to do — App() returns immediately on Android.
}

/// Set minimum window size (no-op on Android).
pub fn set_min_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on Android
}

/// Set maximum window size (no-op on Android).
pub fn set_max_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on Android
}

/// Add a keyboard shortcut.
/// On Android, this is handled via dispatchKeyEvent in the Activity.
/// For now, store the binding and the Activity will check against it.
pub fn add_keyboard_shortcut(_key_ptr: *const u8, _modifiers: f64, _callback: f64) {
    // Stub — Android hardware keyboard shortcuts are uncommon.
    // Could be implemented via onKeyDown in PerryActivity if needed.
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
}

thread_local! {
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
}

/// Set a repeating timer via PerryBridge.setTimer.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let cb_key = crate::callback::register(callback);
    let bridge_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
    });

    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setTimer",
        "(JJ)V",
        &[JValue::Long(cb_key), JValue::Long(interval_ms as i64)],
    );

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}

/// Register callback for app activation (resume).
pub fn on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });
}

/// Register callback for app termination (destroy).
pub fn on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback);
    });
}

/// Called from JNI when Activity resumes.
pub fn handle_activate() {
    ON_ACTIVATE_CALLBACK.with(|c| {
        if let Some(callback) = *c.borrow() {
            let ptr = callback.to_bits() as *const u8;
            unsafe { js_closure_call1(ptr, 0.0); }
        }
    });
}

/// Called from JNI when Activity is destroyed.
pub fn handle_terminate() {
    ON_TERMINATE_CALLBACK.with(|c| {
        if let Some(callback) = *c.borrow() {
            let ptr = callback.to_bits() as *const u8;
            unsafe { js_closure_call1(ptr, 0.0); }
        }
    });
}
