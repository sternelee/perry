use objc2::rc::Retained;
use objc2::msg_send;
use objc2::runtime::AnyClass;
use objc2_ui_kit::UIView;

/// Create a flexible spacer UIView with low content-hugging priority.
/// Works inside UIStackView with Fill distribution: the low hugging priority
/// and explicit zero-height constraint (at low priority) let the stack view
/// expand this view to fill remaining space.
pub fn create() -> i64 {
    unsafe {
        let view: Retained<UIView> = msg_send![
            AnyClass::get(c"UIView").unwrap(),
            new
        ];
        let _: () = msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
        // Set low content hugging priority so spacer stretches in both axes
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 1i64]; // Vertical
        let _: () = msg_send![&*view, setContentHuggingPriority: 1.0f32, forAxis: 0i64]; // Horizontal
        // Set low compression resistance so spacer can be shrunk if needed
        let _: () = msg_send![&*view, setContentCompressionResistancePriority: 1.0f32, forAxis: 1i64];
        let _: () = msg_send![&*view, setContentCompressionResistancePriority: 1.0f32, forAxis: 0i64];

        // Add a zero-height constraint at very low priority. This gives UIStackView's
        // Fill distribution a baseline intrinsic size to work with (plain UIView returns
        // noIntrinsicMetric which confuses the layout engine).
        let h_constraint: Retained<objc2::runtime::AnyObject> = msg_send![
            AnyClass::get(c"NSLayoutConstraint").unwrap(),
            constraintWithItem: &*view,
            attribute: 4i64,       // NSLayoutAttributeHeight
            relatedBy: 0i64,       // NSLayoutRelationEqual
            toItem: std::ptr::null::<objc2::runtime::AnyObject>(),
            attribute: 0i64,       // NSLayoutAttributeNotAnAttribute
            multiplier: 1.0 as objc2_core_foundation::CGFloat,
            constant: 0.0 as objc2_core_foundation::CGFloat
        ];
        let _: () = msg_send![&*h_constraint, setPriority: 1.0f32]; // Very low priority
        let _: () = msg_send![&*h_constraint, setActive: true];

        super::register_widget(view)
    }
}
