use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSString;
use std::cell::RefCell;

thread_local! {
    /// Closure passed to `notificationRegisterRemote(onToken)`. Fires when
    /// `application:didRegisterForRemoteNotificationsWithDeviceToken:` runs.
    static ON_REMOTE_TOKEN_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
    /// Closure passed to `notificationOnReceive(cb)`. Fires for each remote
    /// payload while the app is foregrounded.
    static ON_REMOTE_RECEIVE_CALLBACK: RefCell<Option<f64>> = const { RefCell::new(None) };
}

extern "C" {
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut perry_runtime::string::StringHeader;
    fn js_closure_call1(closure: *const u8, arg0: f64) -> f64;
    fn js_json_parse(text_ptr: *const perry_runtime::string::StringHeader) -> u64;
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).byte_len as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Ask the user for alert + badge + sound permission (options bitmask = 7).
/// Called once from app bootstrap (`PerryAppDelegate.application:didFinishLaunchingWithOptions:`)
/// so the permission prompt fires at launch, not on every `notificationSend` call.
pub fn request_authorization() {
    unsafe {
        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];
        let _: () = msg_send![
            &*center,
            requestAuthorizationWithOptions: 7i64,
            completionHandler: std::ptr::null::<AnyObject>()
        ];
    }
}

/// Store the token callback and ask UIApplication to negotiate an APNs device
/// token. The `application:didRegisterForRemoteNotificationsWithDeviceToken:`
/// delegate method on `PerryAppDelegate` is what actually invokes the closure.
/// Requires the app to be signed with the `aps-environment` entitlement and a
/// matching provisioning profile; without it the fail-to-register delegate
/// fires (logged to stderr).
pub fn register_remote(callback: f64) {
    ON_REMOTE_TOKEN_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
    unsafe {
        let Some(app_cls) = AnyClass::get(c"UIApplication") else { return; };
        let app: *mut AnyObject = msg_send![app_cls, sharedApplication];
        if app.is_null() { return; }
        let _: () = msg_send![app, registerForRemoteNotifications];
    }
}

/// Store the receive callback. Invoked from
/// `application:didReceiveRemoteNotification:` with the payload converted
/// from NSDictionary → JSON → Perry object.
pub fn on_receive(callback: f64) {
    ON_REMOTE_RECEIVE_CALLBACK.with(|c| *c.borrow_mut() = Some(callback));
}

/// Hex-format an APNs device token and invoke the stored closure.
pub unsafe fn dispatch_device_token(device_token: *mut AnyObject) {
    let cb = ON_REMOTE_TOKEN_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else { return; };
    if device_token.is_null() { return; }

    let bytes: *const u8 = msg_send![device_token, bytes];
    let length: usize = msg_send![device_token, length];
    if bytes.is_null() || length == 0 { return; }

    let slice = std::slice::from_raw_parts(bytes, length);
    let mut hex = String::with_capacity(length * 2);
    for b in slice {
        hex.push_str(&format!("{:02X}", b));
    }

    js_stdlib_process_pending();
    js_promise_run_microtasks();

    let str_ptr = js_string_from_bytes(hex.as_ptr(), hex.len() as u32);
    let boxed = js_nanbox_string(str_ptr as i64);
    let ptr = js_nanbox_get_pointer(callback) as *const u8;
    if !ptr.is_null() {
        js_closure_call1(ptr, boxed);
    }
}

/// Log an APNs registration failure.
pub unsafe fn dispatch_registration_failure(error: *mut AnyObject) {
    if error.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed (unknown error)");
        return;
    }
    let desc: *mut AnyObject = msg_send![error, localizedDescription];
    if desc.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed");
        return;
    }
    let utf8: *const u8 = msg_send![desc, UTF8String];
    if utf8.is_null() {
        eprintln!("[perry] registerForRemoteNotifications failed");
        return;
    }
    let len = libc::strlen(utf8 as *const i8);
    let s = std::str::from_utf8_unchecked(std::slice::from_raw_parts(utf8, len));
    eprintln!("[perry] registerForRemoteNotifications failed: {}", s);
}

/// Convert a remote-notification userInfo NSDictionary to a Perry object via
/// NSJSONSerialization + `js_json_parse`, then invoke the stored receive
/// callback with it.
pub unsafe fn dispatch_remote_payload(user_info: *mut AnyObject) {
    let cb = ON_REMOTE_RECEIVE_CALLBACK.with(|c| *c.borrow());
    let Some(callback) = cb else { return; };
    if user_info.is_null() { return; }

    let Some(json_cls) = AnyClass::get(c"NSJSONSerialization") else { return; };
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let data: *mut AnyObject = msg_send![
        json_cls,
        dataWithJSONObject: user_info,
        options: 0u64,
        error: &mut err
    ];
    if data.is_null() { return; }

    let bytes: *const u8 = msg_send![data, bytes];
    let length: usize = msg_send![data, length];
    if bytes.is_null() || length == 0 { return; }

    js_stdlib_process_pending();
    js_promise_run_microtasks();

    let str_ptr = js_string_from_bytes(bytes, length as u32);
    let parsed_bits = js_json_parse(str_ptr);
    let parsed_f64 = f64::from_bits(parsed_bits);

    let ptr = js_nanbox_get_pointer(callback) as *const u8;
    if !ptr.is_null() {
        js_closure_call1(ptr, parsed_f64);
    }
}

/// Send a local notification. Relies on authorization already having been granted
/// via `request_authorization()` at app bootstrap.
pub fn send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);

    unsafe {
        let Some(content_cls) = AnyClass::get(c"UNMutableNotificationContent") else {
            return;
        };
        let content: Retained<AnyObject> = msg_send![content_cls, new];

        let ns_title = NSString::from_str(title);
        let _: () = msg_send![&*content, setTitle: &*ns_title];

        let ns_body = NSString::from_str(body);
        let _: () = msg_send![&*content, setBody: &*ns_body];

        let Some(trigger_cls) = AnyClass::get(c"UNTimeIntervalNotificationTrigger") else {
            return;
        };
        let trigger: Retained<AnyObject> = msg_send![
            trigger_cls,
            triggerWithTimeInterval: 0.1f64,
            repeats: false
        ];

        let Some(request_cls) = AnyClass::get(c"UNNotificationRequest") else {
            return;
        };
        let ident = NSString::from_str("perry_notification");
        let request: Retained<AnyObject> = msg_send![
            request_cls,
            requestWithIdentifier: &*ident,
            content: &*content,
            trigger: &*trigger
        ];

        let Some(center_cls) = AnyClass::get(c"UNUserNotificationCenter") else {
            return;
        };
        let center: Retained<AnyObject> = msg_send![center_cls, currentNotificationCenter];

        let _: () = msg_send![
            &*center,
            addNotificationRequest: &*request,
            withCompletionHandler: std::ptr::null::<AnyObject>()
        ];
    }
}
