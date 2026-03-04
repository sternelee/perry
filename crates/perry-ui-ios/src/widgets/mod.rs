pub mod text;
pub mod button;
pub mod vstack;
pub mod hstack;
pub mod spacer;
pub mod divider;
pub mod textfield;
pub mod securefield;
pub mod toggle;
pub mod slider;
pub mod scrollview;
pub mod canvas;
pub mod progressview;
pub mod image;
pub mod picker;
pub mod form;
pub mod navstack;
pub mod zstack;
pub mod tabbar;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::NSObjectProtocol;
use objc2_ui_kit::{UIView, UIStackView};
use std::cell::RefCell;
use std::ffi::c_void;

thread_local! {
    /// Map from widget handle (1-based) to UIView
    static WIDGETS: RefCell<Vec<Retained<UIView>>> = RefCell::new(Vec::new());
}

/// Store a UIView and return its handle (1-based i64).
pub fn register_widget(view: Retained<UIView>) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(view);
        widgets.len() as i64
    })
}

/// Retrieve the UIView for a given handle.
pub fn get_widget(handle: i64) -> Option<Retained<UIView>> {
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

/// Remove all arranged subviews from a container (UIStackView).
pub fn clear_children(handle: i64) {
    if let Some(parent) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            let subviews = stack.arrangedSubviews();
            for sv in subviews.iter() {
                unsafe {
                    let _: () = objc2::msg_send![stack, removeArrangedSubview: &**sv];
                    sv.removeFromSuperview();
                }
            }
        }
    }
}

/// Add a child view to a parent view at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            unsafe {
                let _: () = objc2::msg_send![stack, insertArrangedSubview: &*child, atIndex: index as usize];
            }
        } else {
            parent.addSubview(&child);
        }
    }
}

/// Add a child view to a parent view.
/// If the parent is a UIStackView, uses addArrangedSubview for proper layout.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"UIStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &UIStackView = unsafe { &*(Retained::as_ptr(&parent) as *const UIStackView) };
            stack.addArrangedSubview(&child);
        } else {
            parent.addSubview(&child);
        }
    }
}

// =============================================================================
// Widget Styling (Background, Gradient, Corner Radius)
// =============================================================================

type CGFloat = f64;

extern "C" {
    fn CGColorRelease(color: *mut c_void);
}

/// Create a CGColor from RGBA via UIColor (iOS doesn't have CGColorCreateGenericRGB).
unsafe fn create_cg_color(r: f64, g: f64, b: f64, a: f64) -> *mut c_void {
    let ui_color: *mut AnyObject = objc2::msg_send![
        AnyClass::get(c"UIColor").unwrap(),
        colorWithRed: r,
        green: g,
        blue: b,
        alpha: a
    ];
    objc2::msg_send![ui_color, CGColor]
}

/// Set a solid background color on any widget.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let ui_color: Retained<AnyObject> = objc2::msg_send![
                AnyClass::get(c"UIColor").unwrap(),
                colorWithRed: r,
                green: g,
                blue: b,
                alpha: a
            ];
            let _: () = objc2::msg_send![&*view, setBackgroundColor: &*ui_color];
        }
    }
}

/// Set a linear gradient background on any widget via CAGradientLayer.
pub fn set_background_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if layer.is_null() { return; }

            // Remove any existing gradient sublayer (tagged by name "PerryGradient")
            let sublayers: *mut AnyObject = objc2::msg_send![layer, sublayers];
            if !sublayers.is_null() {
                let count: usize = objc2::msg_send![sublayers, count];
                let mut i = count;
                while i > 0 {
                    i -= 1;
                    let sub: *mut AnyObject = objc2::msg_send![sublayers, objectAtIndex: i];
                    let name: *mut AnyObject = objc2::msg_send![sub, name];
                    if !name.is_null() {
                        let is_ours: bool = objc2::msg_send![name, isEqualToString:
                            &*objc2_foundation::NSString::from_str("PerryGradient")];
                        if is_ours {
                            let _: () = objc2::msg_send![sub, removeFromSuperlayer];
                        }
                    }
                }
            }

            // Create CAGradientLayer
            let gradient_cls = AnyClass::get(c"CAGradientLayer")
                .expect("CAGradientLayer class not found");
            let gradient: *mut AnyObject = objc2::msg_send![gradient_cls, layer];

            // Set name for later removal
            let name = objc2_foundation::NSString::from_str("PerryGradient");
            let _: () = objc2::msg_send![gradient, setName: &*name];

            // Set frame to match layer bounds
            let bounds: objc2_core_foundation::CGRect = objc2::msg_send![layer, bounds];
            let _: () = objc2::msg_send![gradient, setFrame: bounds];

            // Create colors via UIColor → CGColor
            let color1 = create_cg_color(r1, g1, b1, a1);
            let color2 = create_cg_color(r2, g2, b2, a2);

            // Wrap in NSArray
            let colors: Retained<AnyObject> = {
                let arr_cls = AnyClass::get(c"NSMutableArray").unwrap();
                let arr: *mut AnyObject = objc2::msg_send![arr_cls, arrayWithCapacity: 2usize];
                let _: () = objc2::msg_send![arr, addObject: color1 as *mut AnyObject];
                let _: () = objc2::msg_send![arr, addObject: color2 as *mut AnyObject];
                Retained::retain(arr).unwrap()
            };

            let _: () = objc2::msg_send![gradient, setColors: &*colors];

            // Set direction
            if direction < 0.5 {
                // Vertical: top to bottom
                let start = objc2_core_foundation::CGPoint::new(0.5, 0.0);
                let end = objc2_core_foundation::CGPoint::new(0.5, 1.0);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            } else {
                // Horizontal: left to right
                let start = objc2_core_foundation::CGPoint::new(0.0, 0.5);
                let end = objc2_core_foundation::CGPoint::new(1.0, 0.5);
                let _: () = objc2::msg_send![gradient, setStartPoint: start];
                let _: () = objc2::msg_send![gradient, setEndPoint: end];
            }

            // Insert at index 0 (behind other sublayers)
            let _: () = objc2::msg_send![layer, insertSublayer: gradient, atIndex: 0u32];

            // Auto-resize gradient with the layer
            let mask: u32 = (1 << 1) | (1 << 4); // kCALayerWidthSizable | kCALayerHeightSizable
            let _: () = objc2::msg_send![gradient, setAutoresizingMask: mask];
        }
    }
}

/// Set corner radius on any widget via its layer.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setCornerRadius: radius];
            }
            let _: () = objc2::msg_send![&*view, setClipsToBounds: true];
        }
    }
}

/// Set a fixed width constraint on a widget.
pub fn set_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let width_anchor: Retained<AnyObject> = objc2::msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = objc2::msg_send![
                &*width_anchor, constraintEqualToConstant: width
            ];
            let _: () = objc2::msg_send![&*constraint, setActive: true];
        }
    }
}

/// Set the content hugging priority for both axes.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            // UILayoutConstraintAxis: 0 = Horizontal, 1 = Vertical
            let _: () = objc2::msg_send![&*view, setContentHuggingPriority: priority as f32, forAxis: 0i64];
            let _: () = objc2::msg_send![&*view, setContentHuggingPriority: priority as f32, forAxis: 1i64];
        }
    }
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

use std::collections::HashMap;

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

thread_local! {
    static DOUBLE_CLICK_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

/// Set the enabled state of any UIControl-based widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setEnabled: enabled];
        }
    }
}

/// Set a tooltip (no-op on iOS).
pub fn set_tooltip(_handle: i64, _text: &str) {}

/// Set control size variant (no-op on iOS).
pub fn set_control_size(_handle: i64, _size: i64) {}

/// Animate the opacity of a widget.
pub fn animate_opacity(handle: i64, target: f64, duration_ms: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let duration_secs = duration_ms / 1000.0;
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            let anim_cls = AnyClass::get(c"CABasicAnimation").unwrap();
            let key = objc2_foundation::NSString::from_str("opacity");
            let anim: *mut AnyObject = objc2::msg_send![anim_cls, animationWithKeyPath: &*key];
            let target_ns: Retained<AnyObject> = objc2::msg_send![
                AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: target
            ];
            let _: () = objc2::msg_send![anim, setToValue: &*target_ns];
            let _: () = objc2::msg_send![anim, setDuration: duration_secs];
            let _: () = objc2::msg_send![anim, setFillMode:
                &*objc2_foundation::NSString::from_str("forwards")];
            let _: () = objc2::msg_send![anim, setRemovedOnCompletion: false];
            let _: () = objc2::msg_send![layer, addAnimation: anim, forKey: &*key];
            let _: () = objc2::msg_send![&*view, setAlpha: target];
        }
    }
}

/// Animate the position of a widget by delta.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_ms: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let frame: objc2_core_foundation::CGRect = objc2::msg_send![&*view, frame];
            let new_frame = objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(frame.origin.x + dx, frame.origin.y + dy),
                frame.size,
            );
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            let anim_cls = AnyClass::get(c"CABasicAnimation").unwrap();
            let key = objc2_foundation::NSString::from_str("position");
            let anim: *mut AnyObject = objc2::msg_send![anim_cls, animationWithKeyPath: &*key];
            let _: () = objc2::msg_send![anim, setDuration: duration_ms / 1000.0];
            let _: () = objc2::msg_send![layer, addAnimation: anim, forKey: &*key];
            let _: () = objc2::msg_send![&*view, setFrame: new_frame];
        }
    }
}

/// Set on-hover (no-op on iOS).
pub fn set_on_hover(_handle: i64, _callback: f64) {}

/// Set a double-tap handler.
pub fn set_on_double_click(handle: i64, callback: f64) {
    DOUBLE_CLICK_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });
    if let Some(view) = get_widget(handle) {
        unsafe {
            let gr_cls = AnyClass::get(c"UITapGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![recognizer, init];
            let _: () = objc2::msg_send![recognizer, setNumberOfTapsRequired: 2i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];
        }
    }
}
