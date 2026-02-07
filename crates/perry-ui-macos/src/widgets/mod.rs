pub mod text;
pub mod button;
pub mod vstack;
pub mod hstack;
pub mod spacer;
pub mod divider;
pub mod textfield;
pub mod toggle;
pub mod slider;
pub mod scrollview;

use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2_app_kit::{NSView, NSStackView};
use objc2_foundation::NSObjectProtocol;
use std::cell::RefCell;

thread_local! {
    /// Map from widget handle (1-based) to NSView
    static WIDGETS: RefCell<Vec<Retained<NSView>>> = RefCell::new(Vec::new());
}

/// Store an NSView and return its handle (1-based i64).
pub fn register_widget(view: Retained<NSView>) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(view);
        widgets.len() as i64
    })
}

/// Retrieve the NSView for a given handle.
pub fn get_widget(handle: i64) -> Option<Retained<NSView>> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        widgets.get(idx).cloned()
    })
}

/// Set the hidden state of a widget.
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setHidden: hidden];
        }
    }
}

/// Remove all arranged subviews from a container (NSStackView).
pub fn clear_children(handle: i64) {
    if let Some(parent) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            let subviews = stack.arrangedSubviews();
            for sv in subviews.iter() {
                stack.removeArrangedSubview(&*sv);
                sv.removeFromSuperview();
            }
        }
    }
}

/// Add a child view to a parent view at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            unsafe {
                let _: () = objc2::msg_send![stack, insertArrangedSubview: &*child, atIndex: index as usize];
            }
        } else {
            parent.addSubview(&child);
        }
    }
}

/// Add a child view to a parent view.
/// If the parent is an NSStackView, uses addArrangedSubview for proper layout.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        // Check if parent is an NSStackView
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            // Safety: we verified the type with isKindOfClass
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            stack.addArrangedSubview(&child);
        } else {
            parent.addSubview(&child);
        }
    }
}
