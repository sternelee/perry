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
pub mod alert;
pub mod sheet;
pub mod toolbar;
pub mod lazyvstack;
pub mod table;

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass};
use objc2_app_kit::{NSView, NSStackView};
use objc2_foundation::{NSObject, NSObjectProtocol};
use std::cell::RefCell;

thread_local! {
    /// Map from widget handle (1-based) to NSView
    static WIDGETS: RefCell<Vec<Retained<NSView>>> = RefCell::new(Vec::new());
    /// Stored width constraints per widget handle, so set_width can update instead of duplicate.
    static WIDTH_CONSTRAINTS: RefCell<std::collections::HashMap<i64, Retained<AnyObject>>> = RefCell::new(std::collections::HashMap::new());
    /// Stored height constraints per widget handle, so set_height can update instead of duplicate.
    static HEIGHT_CONSTRAINTS: RefCell<std::collections::HashMap<i64, Retained<AnyObject>>> = RefCell::new(std::collections::HashMap::new());
}

/// Store an NSView and return its handle (1-based i64).
pub fn register_widget(view: Retained<NSView>) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(view);
        widgets.len() as i64
    })
}

/// Register an external NSView (e.g. from a native library) into the widget system.
/// The raw pointer is retained and assigned a handle usable with widgetAddChild etc.
pub fn register_external_nsview(nsview_ptr: i64) -> i64 {
    if nsview_ptr == 0 {
        eprintln!("register_external_nsview: null pointer passed, returning 0");
        return 0;
    }
    match unsafe { Retained::retain(nsview_ptr as *mut NSView) } {
        Some(nsview) => {
            // Disable autoresizing mask constraints so the view can be sized
            // by NSStackView layout instead of being fixed at its initial frame size.
            unsafe {
                let _: () = objc2::msg_send![&*nsview, setTranslatesAutoresizingMaskIntoConstraints: false];
            }
            register_widget(nsview)
        },
        None => {
            eprintln!("register_external_nsview: failed to retain NSView at {:#x}", nsview_ptr);
            0
        }
    }
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

/// Set detachesHiddenViews on an NSStackView.
/// When false, hidden views still participate in layout (occupy space but are invisible).
pub fn set_detaches_hidden_views(handle: i64, detaches: bool) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            unsafe {
                let _: () = msg_send![&*view, setDetachesHiddenViews: detaches];
            }
        }
    }
}

/// Set distribution on an NSStackView.
/// 0 = Fill, 1 = FillEqually, 2 = FillProportionally,
/// 3 = EqualSpacing, 4 = EqualCentering, -1 = GravityAreas.
pub fn set_distribution(handle: i64, distribution: i64) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            unsafe {
                let _: () = msg_send![&*view, setDistribution: distribution];
            }
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

/// Remove a child view from a parent view.
/// If the parent is an NSStackView, removes from arranged subviews first.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent), Some(child)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            stack.removeArrangedSubview(&child);
        }
        child.removeFromSuperview();
    }
}

/// Reorder a child within an NSStackView by moving from one index to another.
pub fn reorder_child(parent_handle: i64, from_index: i64, to_index: i64) {
    if let Some(parent) = get_widget(parent_handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            parent.isKindOfClass(cls)
        } else {
            false
        };

        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&parent) as *const NSStackView) };
            let subviews = stack.arrangedSubviews();
            let count = subviews.len();
            let fi = from_index as usize;
            let ti = to_index as usize;
            if fi < count && ti < count {
                let child: *const NSView = unsafe { objc2::msg_send![&subviews, objectAtIndex: fi] };
                let child_ref: &NSView = unsafe { &*child };
                stack.removeArrangedSubview(child_ref);
                unsafe {
                    let _: () = objc2::msg_send![stack, insertArrangedSubview: child_ref, atIndex: ti];
                }
            }
        }
    }
}

// =============================================================================
// Widget Styling (Background, Gradient, Corner Radius)
// =============================================================================

use std::ffi::c_void;

type CGFloat = f64;

extern "C" {
    fn CGColorCreateGenericRGB(r: CGFloat, g: CGFloat, b: CGFloat, a: CGFloat) -> *mut c_void;
    fn CGColorRelease(color: *mut c_void);
}

/// Set a solid background color on any widget via its layer.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = CGColorCreateGenericRGB(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBackgroundColor: cg_color];
                CGColorRelease(cg_color);
            }
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
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if layer.is_null() { return; }

            // Remove any existing gradient sublayer (tagged by name "PerryGradient")
            let sublayers: *mut AnyObject = objc2::msg_send![layer, sublayers];
            if !sublayers.is_null() {
                let count: usize = objc2::msg_send![sublayers, count];
                // Iterate backwards to safely remove
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

            // Create colors array
            let color1 = CGColorCreateGenericRGB(r1, g1, b1, a1);
            let color2 = CGColorCreateGenericRGB(r2, g2, b2, a2);

            // Wrap in NSArray via obj-c id
            let colors: Retained<AnyObject> = {
                let arr_cls = AnyClass::get(c"NSMutableArray").unwrap();
                let arr: *mut AnyObject = objc2::msg_send![arr_cls, arrayWithCapacity: 2usize];
                let _: () = objc2::msg_send![arr, addObject: color1 as *mut AnyObject];
                let _: () = objc2::msg_send![arr, addObject: color2 as *mut AnyObject];
                Retained::retain(arr).unwrap()
            };

            let _: () = objc2::msg_send![gradient, setColors: &*colors];

            CGColorRelease(color1);
            CGColorRelease(color2);

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

/// Set the border color on any widget via its layer.
pub fn set_border_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let cg_color = CGColorCreateGenericRGB(r, g, b, a);
                let _: () = objc2::msg_send![layer, setBorderColor: cg_color];
                CGColorRelease(cg_color);
            }
        }
    }
}

/// Set the border width on any widget via its layer.
pub fn set_border_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setBorderWidth: width];
            }
        }
    }
}

/// Set edge insets (internal padding) on an NSStackView widget.
/// No-op for non-stack widgets.
pub fn set_edge_insets(handle: i64, top: f64, left: f64, bottom: f64, right: f64) {
    if let Some(view) = get_widget(handle) {
        let is_stack = if let Some(cls) = AnyClass::get(c"NSStackView") {
            view.isKindOfClass(cls)
        } else {
            false
        };
        if is_stack {
            let stack: &NSStackView = unsafe { &*(Retained::as_ptr(&view) as *const NSStackView) };
            unsafe {
                stack.setEdgeInsets(objc2_foundation::NSEdgeInsets { top, left, bottom, right });
            }
        }
    }
}

/// Set the view's alpha (opacity) in [0.0, 1.0].
pub fn set_opacity(handle: i64, alpha: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setAlphaValue: alpha];
        }
    }
}

/// Set corner radius on any widget via its layer.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let layer: *mut AnyObject = objc2::msg_send![&*view, layer];
            if !layer.is_null() {
                let _: () = objc2::msg_send![layer, setCornerRadius: radius];
                let _: () = objc2::msg_send![layer, setMasksToBounds: true];
            }
        }
    }
}

/// Set a fixed width constraint on a widget.
/// Idempotent: deactivates any previous width constraint before creating a new one.
pub fn set_width(handle: i64, width: f64) {
    if let Some(view) = get_widget(handle) {
        // Deactivate old width constraint if any
        WIDTH_CONSTRAINTS.with(|wc| {
            let mut map = wc.borrow_mut();
            if let Some(old) = map.remove(&handle) {
                unsafe {
                    let _: () = msg_send![&*old, setActive: false];
                }
            }
        });
        unsafe {
            let width_anchor: Retained<AnyObject> = msg_send![&*view, widthAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*width_anchor, constraintEqualToConstant: width
            ];
            let _: () = msg_send![&*constraint, setActive: true];
            // Store for future updates
            WIDTH_CONSTRAINTS.with(|wc| {
                wc.borrow_mut().insert(handle, constraint);
            });
        }
    }
}

/// Set a fixed height constraint on a widget.
/// Idempotent: deactivates any previous height constraint before creating a new one.
pub fn set_height(handle: i64, height: f64) {
    if let Some(view) = get_widget(handle) {
        // Deactivate old height constraint if any
        HEIGHT_CONSTRAINTS.with(|hc| {
            let mut map = hc.borrow_mut();
            if let Some(old) = map.remove(&handle) {
                unsafe {
                    let _: () = msg_send![&*old, setActive: false];
                }
            }
        });
        unsafe {
            let height_anchor: Retained<AnyObject> = msg_send![&*view, heightAnchor];
            let constraint: Retained<AnyObject> = msg_send![
                &*height_anchor, constraintEqualToConstant: height
            ];
            let _: () = msg_send![&*constraint, setActive: true];
            // Store for future updates
            HEIGHT_CONSTRAINTS.with(|hc| {
                hc.borrow_mut().insert(handle, constraint);
            });
        }
    }
}

/// Set the content hugging priority for horizontal orientation.
/// Low values (1-250) mean the view is willing to stretch.
/// High values (750-1000) mean the view resists stretching.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if let Some(view) = get_widget(handle) {
        use objc2_app_kit::NSLayoutConstraintOrientation;
        view.setContentHuggingPriority_forOrientation(
            priority as f32, NSLayoutConstraintOrientation::Horizontal);
        view.setContentHuggingPriority_forOrientation(
            priority as f32, NSLayoutConstraintOrientation::Vertical);
    }
}

/// Pin a child view's top and bottom anchors to its superview, forcing it to
/// fill the parent's height.  Useful for HStack children that should stretch
/// vertically instead of being centered.
pub fn match_parent_height(child_handle: i64) {
    if let Some(child) = get_widget(child_handle) {
        unsafe {
            let superview_ptr: *const NSView = msg_send![&*child, superview];
            if superview_ptr.is_null() {
                eprintln!("match_parent_height: view has no superview");
                return;
            }
            let child_top: Retained<AnyObject> = msg_send![&*child, topAnchor];
            let child_bottom: Retained<AnyObject> = msg_send![&*child, bottomAnchor];
            let parent_top: Retained<AnyObject> = msg_send![superview_ptr, topAnchor];
            let parent_bottom: Retained<AnyObject> = msg_send![superview_ptr, bottomAnchor];
            let top_c: Retained<AnyObject> = msg_send![&*child_top, constraintEqualToAnchor: &*parent_top];
            let bot_c: Retained<AnyObject> = msg_send![&*child_bottom, constraintEqualToAnchor: &*parent_bottom];
            let _: () = msg_send![&*top_c, setActive: true];
            let _: () = msg_send![&*bot_c, setActive: true];
        }
    }
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Set the enabled state of any NSControl-based widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setEnabled: enabled];
        }
    }
}

/// Set a tooltip on any widget.
pub fn set_tooltip(handle: i64, text: &str) {
    if let Some(view) = get_widget(handle) {
        let ns_text = objc2_foundation::NSString::from_str(text);
        unsafe {
            let _: () = objc2::msg_send![&*view, setToolTip: &*ns_text];
        }
    }
}

/// Set control size variant on NSControl-based widgets.
/// 0=regular, 1=small, 2=mini, 3=large
pub fn set_control_size(handle: i64, size: i64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setControlSize: size as u64];
        }
    }
}

/// Animate the opacity of a widget.
pub fn animate_opacity(handle: i64, target: f64, duration_ms: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let _: () = objc2::msg_send![&*view, setWantsLayer: true];
            let duration_secs = duration_ms / 1000.0;
            // Use NSAnimationContext
            let ctx_cls = AnyClass::get(c"NSAnimationContext").unwrap();
            let _: () = objc2::msg_send![ctx_cls, beginGrouping];
            let ctx: *mut AnyObject = objc2::msg_send![ctx_cls, currentContext];
            let _: () = objc2::msg_send![ctx, setDuration: duration_secs];
            let _: () = objc2::msg_send![ctx, setAllowsImplicitAnimation: true];
            let _: () = objc2::msg_send![&*view, setAlphaValue: target];
            let _: () = objc2::msg_send![ctx_cls, endGrouping];
        }
    }
}

/// Animate the position of a widget by delta.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_ms: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            let duration_secs = duration_ms / 1000.0;
            let frame: objc2_core_foundation::CGRect = objc2::msg_send![&*view, frame];
            let new_origin = objc2_core_foundation::CGPoint::new(
                frame.origin.x + dx,
                frame.origin.y + dy,
            );
            let ctx_cls = AnyClass::get(c"NSAnimationContext").unwrap();
            let _: () = objc2::msg_send![ctx_cls, beginGrouping];
            let ctx: *mut AnyObject = objc2::msg_send![ctx_cls, currentContext];
            let _: () = objc2::msg_send![ctx, setDuration: duration_secs];
            let _: () = objc2::msg_send![ctx, setAllowsImplicitAnimation: true];
            let _: () = objc2::msg_send![&*view, setFrameOrigin: new_origin];
            let _: () = objc2::msg_send![ctx_cls, endGrouping];
        }
    }
}

use std::collections::HashMap;

thread_local! {
    static HOVER_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static DOUBLE_CLICK_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
    static CLICK_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

/// Set an on-hover callback for a widget (mouse enter/exit).
pub fn set_on_hover(handle: i64, callback: f64) {
    HOVER_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });
    if let Some(view) = get_widget(handle) {
        unsafe {
            // Add tracking area for mouse enter/exit
            let ta_cls = AnyClass::get(c"NSTrackingArea").unwrap();
            let bounds: objc2_core_foundation::CGRect = objc2::msg_send![&*view, bounds];
            let options: u64 = 0x01 | 0x02 | 0x20; // MouseEnteredAndExited | MouseMoved | ActiveAlways
            let tracking_area: *mut AnyObject = objc2::msg_send![
                ta_cls, alloc
            ];
            let tracking_area: *mut AnyObject = objc2::msg_send![
                tracking_area, initWithRect: bounds, options: options, owner: &*view, userInfo: std::ptr::null::<AnyObject>()
            ];
            let _: () = objc2::msg_send![&*view, addTrackingArea: tracking_area];
        }
    }
}

/// Set a double-click handler for a widget.
pub fn set_on_double_click(handle: i64, callback: f64) {
    DOUBLE_CLICK_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(handle, callback);
    });
    if let Some(view) = get_widget(handle) {
        unsafe {
            let gr_cls = AnyClass::get(c"NSClickGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![recognizer, init];
            let _: () = objc2::msg_send![recognizer, setNumberOfClicksRequired: 2i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];
        }
    }
}

// =============================================================================
// Single-click handler for any widget
// =============================================================================

/// Internal state for click gesture target
pub struct PerryClickTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

objc2::define_class!(
    #[unsafe(super(objc2_foundation::NSObject))]
    #[name = "PerryClickTarget"]
    #[ivars = PerryClickTargetIvars]
    pub struct PerryClickTarget;

    impl PerryClickTarget {
        #[unsafe(method(handleClick:))]
        fn handle_click(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("click callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = CLICK_CALLBACKS.with(|cbs| {
                    cbs.borrow().get(&key).copied()
                });
                if let Some(closure_f64) = closure_f64 {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            }));
        }
    }
);

impl PerryClickTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryClickTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { objc2::msg_send![super(this), init] }
    }
}

/// Set a single-click handler for any widget.
pub fn set_on_click(handle: i64, callback: f64) {
    if let Some(view) = get_widget(handle) {
        unsafe {
            // Create target object for the gesture recognizer
            let target = PerryClickTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            // Store callback keyed by target address
            CLICK_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            // Create NSClickGestureRecognizer with target-action
            let sel = objc2::runtime::Sel::register(c"handleClick:");
            let gr_cls = AnyClass::get(c"NSClickGestureRecognizer").unwrap();
            let recognizer: *mut AnyObject = objc2::msg_send![gr_cls, alloc];
            let recognizer: *mut AnyObject = objc2::msg_send![
                recognizer, initWithTarget: &*target, action: sel
            ];
            let _: () = objc2::msg_send![recognizer, setNumberOfClicksRequired: 1i64];
            let _: () = objc2::msg_send![&*view, addGestureRecognizer: recognizer];

            // Leak target to keep it alive
            std::mem::forget(target);
        }
    }
}
