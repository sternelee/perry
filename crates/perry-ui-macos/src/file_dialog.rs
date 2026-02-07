use objc2::msg_send;
use objc2_app_kit::NSOpenPanel;
use objc2_foundation::{MainThreadMarker, NSString};

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
    fn js_nanbox_string(ptr: i64) -> f64;
}

/// Open a file dialog. Calls callback with the selected file path (NaN-boxed string).
/// If user cancels, callback is called with TAG_UNDEFINED.
pub fn open_dialog(callback: f64) {
    let _mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    unsafe {
        let panel: objc2::rc::Retained<NSOpenPanel> = msg_send![
            objc2::runtime::AnyClass::get(c"NSOpenPanel").unwrap(),
            openPanel
        ];
        panel.setCanChooseFiles(true);
        panel.setCanChooseDirectories(false);
        panel.setAllowsMultipleSelection(false);

        // Run modal (blocks until user responds)
        let response: isize = msg_send![&*panel, runModal];
        let closure_ptr = js_nanbox_get_pointer(callback) as *const u8;

        if response == 1 {
            // NSModalResponseOK = 1
            let urls = panel.URLs();
            if !urls.is_empty() {
                let url = &urls.objectAtIndex(0);
                let path_str: objc2::rc::Retained<NSString> = msg_send![url, path];
                let rust_str = path_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                let nanboxed = js_nanbox_string(str_ptr as i64);
                js_closure_call1(closure_ptr, nanboxed);
            } else {
                js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
            }
        } else {
            // User cancelled
            js_closure_call1(closure_ptr, f64::from_bits(0x7FFC_0000_0000_0001));
        }
    }
}
