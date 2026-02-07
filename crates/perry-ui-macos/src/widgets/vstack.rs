use objc2::rc::Retained;
use objc2_app_kit::{NSStackView, NSView, NSUserInterfaceLayoutOrientation, NSLayoutAttribute, NSStackViewGravity};
use objc2_foundation::MainThreadMarker;

/// Create an NSStackView with vertical orientation.
pub fn create(spacing: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(spacing);
    stack.setAlignment(NSLayoutAttribute::CenterX);
    // Edge insets for padding
    unsafe {
        stack.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top: 20.0, left: 20.0, bottom: 20.0, right: 20.0,
        });
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(stack) };
    super::register_widget(view)
}

/// Create an NSStackView with vertical orientation and custom edge insets.
pub fn create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let stack = NSStackView::new(mtm);
    stack.setOrientation(NSUserInterfaceLayoutOrientation::Vertical);
    stack.setSpacing(spacing);
    stack.setAlignment(NSLayoutAttribute::CenterX);
    unsafe {
        stack.setEdgeInsets(objc2_foundation::NSEdgeInsets {
            top, left, bottom, right,
        });
    }
    let view: Retained<NSView> = unsafe { Retained::cast_unchecked(stack) };
    super::register_widget(view)
}
