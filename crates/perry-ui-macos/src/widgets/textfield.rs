use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObject, NSString, MainThreadMarker};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from observer object address to (closure_f64, textfield_view_ptr)
    static TEXTFIELD_CALLBACKS: RefCell<HashMap<usize, (f64, *const AnyObject)>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Internal state for the notification observer
pub struct PerryTextFieldObserverIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTextFieldObserver"]
    #[ivars = PerryTextFieldObserverIvars]
    pub struct PerryTextFieldObserver;

    impl PerryTextFieldObserver {
        #[unsafe(method(textDidChange:))]
        fn text_did_change(&self, notification: &NSNotification) {
            let key = self.ivars().callback_key.get();
            TEXTFIELD_CALLBACKS.with(|cbs| {
                if let Some(&(closure_f64, tf_ptr)) = cbs.borrow().get(&key) {
                    if tf_ptr.is_null() {
                        return;
                    }

                    // Check the notification object matches our text field
                    let notif_obj = notification.object();
                    if let Some(obj) = notif_obj {
                        let obj_ptr = &*obj as *const AnyObject;
                        if obj_ptr != tf_ptr {
                            return; // Not our text field
                        }
                    } else {
                        return;
                    }

                    let text_field = tf_ptr as *const NSTextField;
                    let text: Retained<NSString> = unsafe { (*text_field).stringValue() };
                    let rust_str = text.to_string();
                    let bytes = rust_str.as_bytes();

                    // Create a StringHeader-backed string and NaN-box it
                    let str_ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64) };
                    let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };

                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call1(closure_ptr as *const u8, nanboxed);
                    }
                }
            });
        }
    }
);

impl PerryTextFieldObserver {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTextFieldObserverIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
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

/// Create an editable NSTextField with a placeholder string and onChange callback.
/// `placeholder_ptr` is a StringHeader pointer, `on_change` is a NaN-boxed closure.
pub fn create(placeholder_ptr: *const u8, on_change: f64) -> i64 {
    let placeholder = str_from_header(placeholder_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_placeholder = NSString::from_str(placeholder);

    unsafe {
        let text_field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        text_field.setPlaceholderString(Some(&ns_placeholder));

        // Make it editable
        text_field.setEditable(true);
        text_field.setBezeled(true);

        let view: Retained<NSView> = Retained::cast_unchecked(text_field);
        let handle = super::register_widget(view);

        // Get the raw pointer to the text field for notification matching
        let tf_view = super::get_widget(handle).unwrap();
        let tf_raw: *const AnyObject = Retained::as_ptr(&tf_view) as *const AnyObject;

        // Set up notification observer for text changes
        let observer = PerryTextFieldObserver::new();
        let observer_addr = Retained::as_ptr(&observer) as usize;
        observer.ivars().callback_key.set(observer_addr);

        TEXTFIELD_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(observer_addr, (on_change, tf_raw));
        });

        // Register for NSControlTextDidChangeNotification
        let center = NSNotificationCenter::defaultCenter();
        let notif_name = NSString::from_str("NSControlTextDidChangeNotification");
        let sel = Sel::register(c"textDidChange:");
        let _: () = msg_send![&center, addObserver: &*observer, selector: sel, name: &*notif_name, object: tf_raw];

        // Prevent observer from being deallocated
        std::mem::forget(observer);

        handle
    }
}

/// Focus an editable text field (make it first responder).
pub fn focus(handle: i64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            if let Some(window) = tf.window() {
                window.makeFirstResponder(Some(tf));
            }
        }
    }
}

/// Set the text of an editable text field from a StringHeader pointer.
pub fn set_string_value(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setStringValue(&ns_string);
        }
    }
}
