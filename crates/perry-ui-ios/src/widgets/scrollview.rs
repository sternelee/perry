use objc2::rc::Retained;
use objc2::msg_send;
use objc2_ui_kit::{UIScrollView, UIView};
use objc2_core_foundation::CGPoint;

/// Create a UIScrollView. Returns widget handle.
pub fn create() -> i64 {
    unsafe {
        let scroll: Retained<UIScrollView> = msg_send![
            objc2::runtime::AnyClass::get(c"UIScrollView").unwrap(),
            new
        ];
        let _: () = msg_send![&*scroll, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Disable touch delay to avoid iOS 26 crash in
        // UIGestureRecognizer _delayTouchesForEvent:inPhase:
        let _: () = msg_send![&*scroll, setDelaysContentTouches: false];
        // Dismiss keyboard when user scrolls (UIScrollViewKeyboardDismissModeOnDrag = 1)
        let _: () = msg_send![&*scroll, setKeyboardDismissMode: 1i64];
        // Automatically adjust content inset for safe area (status bar, home indicator)
        // UIScrollViewContentInsetAdjustmentAlways = 2
        let _: () = msg_send![&*scroll, setContentInsetAdjustmentBehavior: 2i64];

        let view: Retained<UIView> = Retained::cast_unchecked(scroll);
        super::register_widget(view)
    }
}

/// Set the content child of a UIScrollView.
pub fn set_child(scroll_handle: i64, child_handle: i64) {
    if let (Some(scroll_view), Some(child)) = (super::get_widget(scroll_handle), super::get_widget(child_handle)) {
        unsafe {
            // Add child as subview of the scroll view
            scroll_view.addSubview(&child);

            // Pin child to scroll view's content layout guide
            let content_guide: *const objc2::runtime::AnyObject = msg_send![&*scroll_view, contentLayoutGuide];

            let child_leading: *const objc2::runtime::AnyObject = msg_send![&*child, leadingAnchor];
            let child_trailing: *const objc2::runtime::AnyObject = msg_send![&*child, trailingAnchor];
            let child_top: *const objc2::runtime::AnyObject = msg_send![&*child, topAnchor];
            let child_bottom: *const objc2::runtime::AnyObject = msg_send![&*child, bottomAnchor];

            let guide_leading: *const objc2::runtime::AnyObject = msg_send![content_guide, leadingAnchor];
            let guide_trailing: *const objc2::runtime::AnyObject = msg_send![content_guide, trailingAnchor];
            let guide_top: *const objc2::runtime::AnyObject = msg_send![content_guide, topAnchor];
            let guide_bottom: *const objc2::runtime::AnyObject = msg_send![content_guide, bottomAnchor];

            let c1: Retained<objc2::runtime::AnyObject> = msg_send![child_leading, constraintEqualToAnchor: guide_leading];
            let c2: Retained<objc2::runtime::AnyObject> = msg_send![child_trailing, constraintEqualToAnchor: guide_trailing];
            let c3: Retained<objc2::runtime::AnyObject> = msg_send![child_top, constraintEqualToAnchor: guide_top];
            let c4: Retained<objc2::runtime::AnyObject> = msg_send![child_bottom, constraintEqualToAnchor: guide_bottom];

            let _: () = msg_send![&*c1, setActive: true];
            let _: () = msg_send![&*c2, setActive: true];
            let _: () = msg_send![&*c3, setActive: true];
            let _: () = msg_send![&*c4, setActive: true];

            // Match width to scroll view's frame layout guide
            let frame_guide: *const objc2::runtime::AnyObject = msg_send![&*scroll_view, frameLayoutGuide];
            let child_width: *const objc2::runtime::AnyObject = msg_send![&*child, widthAnchor];
            let frame_width: *const objc2::runtime::AnyObject = msg_send![frame_guide, widthAnchor];
            let cw: Retained<objc2::runtime::AnyObject> = msg_send![child_width, constraintEqualToAnchor: frame_width];
            let _: () = msg_send![&*cw, setActive: true];
        }
    }
}

/// Scroll so that the given child widget is visible.
pub fn scroll_to(_scroll_handle: i64, child_handle: i64) {
    if let Some(child) = super::get_widget(child_handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = msg_send![&*child, frame];
            // Find the scroll view parent
            let superview: *const objc2::runtime::AnyObject = msg_send![&*child, superview];
            if !superview.is_null() {
                let _: () = msg_send![superview, scrollRectToVisible: frame, animated: true];
            }
        }
    }
}

/// Get the vertical scroll offset (contentOffset.y).
pub fn get_offset(scroll_handle: i64) -> f64 {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let offset: CGPoint = msg_send![&*scroll_view, contentOffset];
            offset.y
        }
    } else {
        0.0
    }
}

/// Set the vertical scroll offset.
pub fn set_offset(scroll_handle: i64, offset: f64) {
    if let Some(scroll_view) = super::get_widget(scroll_handle) {
        unsafe {
            let point = CGPoint::new(0.0, offset);
            let _: () = msg_send![&*scroll_view, setContentOffset: point, animated: true];
        }
    }
}
