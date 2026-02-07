use objc2::rc::Retained;
use objc2_app_kit::{NSTextField, NSView};
use objc2_foundation::{NSString, MainThreadMarker};
use perry_runtime::string::StringHeader;

use super::register_widget;

/// Extract a &str from a *const StringHeader pointer.
/// StringHeader is { length: u32, capacity: u32 } followed by UTF-8 data.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const StringHeader;
        let len = (*header).length as usize;
        let data = ptr.add(std::mem::size_of::<StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create an NSTextField configured as a non-editable label.
pub fn create(text_ptr: *const u8) -> i64 {
    let text = str_from_header(text_ptr);

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let ns_string = NSString::from_str(text);

    let label = NSTextField::labelWithString(&ns_string, mtm);
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(label) };
    register_widget(view)
}

/// Update the text of an existing Text widget (NSTextField).
pub fn set_text_str(handle: i64, text: &str) {
    if let Some(view) = super::get_widget(handle) {
        let ns_string = NSString::from_str(text);
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setStringValue(&ns_string);
        }
    }
}

/// Update the text of an existing Text widget from a StringHeader pointer.
pub fn set_string(handle: i64, text_ptr: *const u8) {
    let text = str_from_header(text_ptr);
    set_text_str(handle, text);
}

/// Set the text color of a Text widget.
pub fn set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let color: Retained<objc2_app_kit::NSColor> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r as objc2_core_foundation::CGFloat,
                green: g as objc2_core_foundation::CGFloat,
                blue: b as objc2_core_foundation::CGFloat,
                alpha: a as objc2_core_foundation::CGFloat
            ];
            tf.setTextColor(Some(&color));
        }
    }
}

/// Set the font size of a Text widget.
pub fn set_font_size(handle: i64, size: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let font: Retained<objc2_app_kit::NSFont> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat
            ];
            tf.setFont(Some(&font));
        }
    }
}

/// Set the font weight of a Text widget (0.0 = regular, 1.0 = bold).
pub fn set_font_weight(handle: i64, size: f64, weight: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            let font: Retained<objc2_app_kit::NSFont> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSFont").unwrap(),
                systemFontOfSize: size as objc2_core_foundation::CGFloat,
                weight: weight as objc2_core_foundation::CGFloat
            ];
            tf.setFont(Some(&font));
        }
    }
}

/// Set whether a Text widget is selectable.
pub fn set_selectable(handle: i64, selectable: bool) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let tf: &NSTextField = &*(Retained::as_ptr(&view) as *const NSTextField);
            tf.setSelectable(selectable);
        }
    }
}
