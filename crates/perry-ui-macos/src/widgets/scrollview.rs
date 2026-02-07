use objc2::rc::Retained;
use objc2::msg_send;
use objc2_app_kit::{NSScrollView, NSView};
use objc2_core_foundation::CGPoint;
use objc2_foundation::MainThreadMarker;

/// Create an NSScrollView with vertical scrollbar. Returns widget handle.
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let scroll = NSScrollView::new(mtm);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(scroll) };
    super::register_widget(view)
}

/// Set the document (content) view of a scroll view.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (super::get_widget(scroll_handle), super::get_widget(child_handle)) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            sv.setDocumentView(Some(&child));
        }
    }
}

/// Scroll so that the given child widget is visible.
pub fn scroll_to(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (super::get_widget(scroll_handle), super::get_widget(child_handle)) {
        unsafe {
            let _sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let child_frame: objc2_core_foundation::CGRect = msg_send![&*child, frame];
            let _: () = msg_send![&*child, scrollRectToVisible: child_frame];
        }
    }
}

/// Get the vertical scroll offset (contentView.bounds.origin.y).
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let content_view = sv.contentView();
            let bounds: objc2_core_foundation::CGRect = msg_send![&*content_view, bounds];
            bounds.origin.y
        }
    } else {
        0.0
    }
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let sv: &NSScrollView = &*(Retained::as_ptr(&scroll_view) as *const NSScrollView);
            let content_view = sv.contentView();
            let point = CGPoint::new(0.0, offset);
            let _: () = msg_send![&*content_view, setBoundsOrigin: point];
        }
    }
}
