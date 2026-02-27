use std::cell::RefCell;
use std::collections::HashMap;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_closure_call2(closure: *const u8, arg1: f64, arg2: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

thread_local! {
    /// Maps callback key (i64) to NaN-boxed closure f64.
    static CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static NEXT_KEY: RefCell<i64> = RefCell::new(1);
}

/// Register a NaN-boxed closure and return a unique key for it.
pub fn register(closure_f64: f64) -> i64 {
    NEXT_KEY.with(|k| {
        let key = *k.borrow();
        *k.borrow_mut() = key + 1;
        CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(key, closure_f64);
        });
        unsafe {
            __android_log_print(
                3, b"PerryCallback\0".as_ptr(),
                b"register: key=%lld bits=0x%llx\0".as_ptr(),
                key as i64, closure_f64.to_bits() as i64,
            );
        }
        key
    })
}

/// Invoke a registered callback with 0 arguments.
/// IMPORTANT: Extract closure_f64 and DROP the RefCell borrow BEFORE calling
/// js_closure_call0. The closure may re-enter callback::register() which needs
/// to borrow_mut() CALLBACKS. (Same fix as iOS re-entrant borrow issue.)
pub fn invoke0(key: i64) {
    let closure_f64 = CALLBACKS.with(|cbs| {
        cbs.borrow().get(&key).copied()
    });
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            __android_log_print(
                3, b"PerryCallback\0".as_ptr(),
                b"invoke0: key=%lld ptr=%p\0".as_ptr(),
                key as i64, closure_ptr,
            );
            js_closure_call0(closure_ptr);
            __android_log_print(
                3, b"PerryCallback\0".as_ptr(),
                b"invoke0: closure returned for key=%lld\0".as_ptr(),
                key as i64,
            );
        }
    } else {
        unsafe {
            __android_log_print(
                3, b"PerryCallback\0".as_ptr(),
                b"invoke0: key=%lld NOT FOUND\0".as_ptr(),
                key as i64,
            );
        }
    }
}

/// Invoke a registered callback with 1 argument.
/// Same re-entrant borrow fix as invoke0.
pub fn invoke1(key: i64, arg: f64) {
    let closure_f64 = CALLBACKS.with(|cbs| {
        cbs.borrow().get(&key).copied()
    });
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            js_closure_call1(closure_ptr, arg);
        }
    }
}

/// Invoke a registered callback with 2 arguments.
/// Same re-entrant borrow fix as invoke0.
pub fn invoke2(key: i64, arg1: f64, arg2: f64) {
    let closure_f64 = CALLBACKS.with(|cbs| {
        cbs.borrow().get(&key).copied()
    });
    if let Some(closure_f64) = closure_f64 {
        let closure_ptr = closure_f64.to_bits() as *const u8;
        unsafe {
            js_closure_call2(closure_ptr, arg1, arg2);
        }
    }
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback0(long key).
/// This runs on the UI thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback0(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
) {
    invoke0(key as i64);
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback1(long key, double arg).
/// This runs on the UI thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback1(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    arg: jni::sys::jdouble,
) {
    invoke1(key as i64, arg);
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallback2(long key, double arg1, double arg2).
/// This runs on the UI thread.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallback2(
    _env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    arg1: jni::sys::jdouble,
    arg2: jni::sys::jdouble,
) {
    invoke2(key as i64, arg1, arg2);
}

/// JNI entry point: called from Java PerryBridge.nativeInvokeCallbackWithString(long key, String text).
/// Converts the Java String to a NaN-boxed Perry string and invokes the callback.
#[no_mangle]
pub extern "C" fn Java_com_perry_app_PerryBridge_nativeInvokeCallbackWithString(
    mut env: jni::JNIEnv,
    _class: jni::objects::JClass,
    key: jni::sys::jlong,
    text: jni::objects::JString,
) {
    let rust_str: String = env.get_string(&text)
        .map(|s| s.into())
        .unwrap_or_default();
    let bytes = rust_str.as_bytes();
    let nanboxed = unsafe {
        let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
        js_nanbox_string(str_ptr as i64)
    };
    invoke1(key as i64, nanboxed);
}
