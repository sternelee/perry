use gtk4::prelude::*;
use gtk4::Button;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Map from button ID to closure pointer (f64 NaN-boxed)
    static BUTTON_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static NEXT_BUTTON_ID: RefCell<usize> = RefCell::new(1);
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

/// Create a GtkButton with a label and closure callback.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    crate::app::ensure_gtk_init();
    let label = str_from_header(label_ptr);
    let button = Button::with_label(label);

    let callback_id = NEXT_BUTTON_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    BUTTON_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(callback_id, on_press);
    });

    button.connect_clicked(move |_btn| {
        let closure_f64 = BUTTON_CALLBACKS.with(|cbs| {
            cbs.borrow().get(&callback_id).copied()
        });
        if let Some(closure_f64) = closure_f64 {
            let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
            unsafe {
                js_closure_call0(closure_ptr as *const u8);
            }
        }
    });

    super::register_widget(button.upcast())
}

/// Set whether a button has a visible border/frame.
pub fn set_bordered(handle: i64, bordered: bool) {
    if let Some(widget) = super::get_widget(handle) {
        if let Some(button) = widget.downcast_ref::<Button>() {
            if bordered {
                button.remove_css_class("flat");
            } else {
                button.add_css_class("flat");
            }
        }
    }
}

/// Set the title text of a button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(widget) = super::get_widget(handle) {
        if let Some(button) = widget.downcast_ref::<Button>() {
            button.set_label(title);
        }
    }
}

/// Set the text color of a button's label via CSS.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(widget) = super::get_widget(handle) {
        let css = format!(
            "button label {{ color: rgba({},{},{},{}); }}",
            (r * 255.0) as u8,
            (g * 255.0) as u8,
            (b * 255.0) as u8,
            a
        );
        super::apply_css(&widget, &css);
    }
}
