use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, ClassType, DefinedClass, MainThreadOnly};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use objc2_ui_kit::{UIView, UIViewController, UIWindow};

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CStr;

use crate::widgets;
use crate::menu;

thread_local! {
    static PENDING_CONFIG: RefCell<Option<AppConfig>> = RefCell::new(None);
    static PENDING_BODY: RefCell<Option<i64>> = RefCell::new(None);
    static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
}

struct AppConfig {
    title: String,
    _width: f64,
    _height: f64,
}

struct AppEntry {
    window: Retained<UIWindow>,
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

/// Create an app. Stores config in thread-local for deferred creation.
/// Returns app handle (i64).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App".to_string()
    } else {
        str_from_header(title_ptr).to_string()
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    PENDING_CONFIG.with(|c| {
        *c.borrow_mut() = Some(AppConfig {
            title,
            _width: w,
            _height: h,
        });
    });

    1 // Single app handle
}

/// Set the root widget (body) of the app.
pub fn app_set_body(_app_handle: i64, root_handle: i64) {
    PENDING_BODY.with(|b| {
        *b.borrow_mut() = Some(root_handle);
    });
}

/// Define the PerryAppDelegate class for UIApplicationDelegate protocol.
pub struct PerryAppDelegateIvars {}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryAppDelegate"]
    #[ivars = PerryAppDelegateIvars]
    pub struct PerryAppDelegate;

    impl PerryAppDelegate {
        #[unsafe(method(application:didFinishLaunchingWithOptions:))]
        fn did_finish_launching(&self, _application: &AnyObject, _options: *const AnyObject) -> bool {
            // Window creation is handled by PerrySceneDelegate
            true
        }
    }
);

/// Scene delegate callback: creates the UIWindow and attaches the root widget.
/// Called by UIKit when the scene connects.
unsafe extern "C" fn scene_will_connect(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    scene: *mut AnyObject,
    _session: *mut AnyObject,
    _options: *mut AnyObject,
) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    // Create UIWindow attached to the scene (UIWindowScene)
    let window_cls = AnyClass::get(c"UIWindow").unwrap();
    let window_alloc: *mut AnyObject = msg_send![window_cls, alloc];
    let window_raw: *mut AnyObject = msg_send![window_alloc, initWithWindowScene: scene];
    let window: Retained<UIWindow> = Retained::cast_unchecked(Retained::retain(window_raw as *mut AnyObject).unwrap());

    // Create root PerryViewController (custom subclass with menu bar support).
    // Falls back to UIViewController if PerryViewController isn't registered yet.
    let vc_cls = AnyClass::get(c"PerryViewController")
        .unwrap_or_else(|| AnyClass::get(c"UIViewController").unwrap());
    let vc: Retained<UIViewController> = msg_send![vc_cls, new];

    // Set white background
    let white: Retained<AnyObject> = msg_send![
        AnyClass::get(c"UIColor").unwrap(),
        whiteColor
    ];
    let vc_view: Retained<UIView> = msg_send![&*vc, view];
    let _: () = msg_send![&*vc_view, setBackgroundColor: &*white];

    // Attach the root widget if set
    PENDING_BODY.with(|b| {
        if let Some(root_handle) = b.borrow().as_ref() {
            if let Some(root_view) = widgets::get_widget(*root_handle) {
                let _: () = msg_send![&*root_view, setTranslatesAutoresizingMaskIntoConstraints: false];

                vc_view.addSubview(&root_view);

                // Pin root widget to view edges (not safe area) for edge-to-edge layout
                let root_leading: *const AnyObject = msg_send![&*root_view, leadingAnchor];
                let root_trailing: *const AnyObject = msg_send![&*root_view, trailingAnchor];
                let root_top: *const AnyObject = msg_send![&*root_view, topAnchor];
                let root_bottom: *const AnyObject = msg_send![&*root_view, bottomAnchor];

                let view_leading: *const AnyObject = msg_send![&*vc_view, leadingAnchor];
                let view_trailing: *const AnyObject = msg_send![&*vc_view, trailingAnchor];
                let view_top: *const AnyObject = msg_send![&*vc_view, topAnchor];
                let view_bottom: *const AnyObject = msg_send![&*vc_view, bottomAnchor];

                let c1: Retained<AnyObject> = msg_send![root_leading, constraintEqualToAnchor: view_leading];
                let c2: Retained<AnyObject> = msg_send![root_trailing, constraintEqualToAnchor: view_trailing];
                let c3: Retained<AnyObject> = msg_send![root_top, constraintEqualToAnchor: view_top];
                let c4: Retained<AnyObject> = msg_send![root_bottom, constraintEqualToAnchor: view_bottom];

                let _: () = msg_send![&*c1, setActive: true];
                let _: () = msg_send![&*c2, setActive: true];
                let _: () = msg_send![&*c3, setActive: true];
                let _: () = msg_send![&*c4, setActive: true];
            }
        }
    });

    window.setRootViewController(Some(&vc));
    window.makeKeyAndVisible();

    APPS.with(|a| {
        a.borrow_mut().push(AppEntry { window });
    });

    // Start the timer pump to drive setInterval/setTimeout callbacks (8ms ≈ 120Hz).
    // Without this, js_interval_timer_tick() is never called and setInterval never fires.
    let pump_target = PerryPumpTarget::new();
    let pump_sel = Sel::register(c"pump:");
    let _: Retained<AnyObject> = msg_send![
        objc2::class!(NSTimer),
        scheduledTimerWithTimeInterval: 0.008f64,
        target: &*pump_target,
        selector: pump_sel,
        userInfo: std::ptr::null::<AnyObject>(),
        repeats: true
    ];
    std::mem::forget(pump_target);
}

// Raw ObjC runtime FFI for dynamic class registration
extern "C" {
    fn objc_allocateClassPair(superclass: *const std::ffi::c_void, name: *const i8, extra_bytes: usize) -> *mut std::ffi::c_void;
    fn objc_registerClassPair(cls: *mut std::ffi::c_void);
    fn class_addMethod(cls: *mut std::ffi::c_void, sel: *const std::ffi::c_void, imp: *const std::ffi::c_void, types: *const i8) -> bool;
    fn class_addProtocol(cls: *mut std::ffi::c_void, protocol: *const std::ffi::c_void) -> bool;
    fn sel_registerName(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getClass(name: *const i8) -> *const std::ffi::c_void;
    fn objc_getProtocol(name: *const i8) -> *const std::ffi::c_void;
}

/// Register the PerrySceneDelegate class dynamically at runtime.
fn register_scene_delegate() {
    unsafe {
        let superclass = objc_getClass(c"UIResponder".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerrySceneDelegate".as_ptr(), 0);
        if cls.is_null() {
            // Class already registered
            return;
        }

        // Declare conformance to UIWindowSceneDelegate (which inherits UISceneDelegate).
        // iOS 26+ requires formal protocol conformance, not just method presence.
        let proto = objc_getProtocol(c"UIWindowSceneDelegate".as_ptr());
        if !proto.is_null() {
            class_addProtocol(cls, proto);
        }
        let proto2 = objc_getProtocol(c"UISceneDelegate".as_ptr());
        if !proto2.is_null() {
            class_addProtocol(cls, proto2);
        }

        // Add scene:willConnectToSession:options: method
        // ObjC type encoding: v@:@@@ (void, self, _cmd, scene, session, options)
        let sel = sel_registerName(c"scene:willConnectToSession:options:".as_ptr());
        class_addMethod(
            cls,
            sel,
            scene_will_connect as *const std::ffi::c_void,
            c"v@:@@@".as_ptr(),
        );

        objc_registerClassPair(cls);
    }
}

// ─── PerryViewController ─────────────────────────────────────────────────────
// A custom UIViewController subclass that overrides:
//   - buildMenuWithBuilder: — populates the iPadOS menu bar via UIMenuBuilder
//   - perryMenuAction:      — dispatches UICommand/UIKeyCommand taps to JS callbacks

/// buildMenuWithBuilder: callback — forwards to menu::build_menubar_for_builder.
unsafe extern "C" fn vc_build_menu(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    builder: *mut AnyObject,
) {
    // Call super first so UIKit fills in system menus
    // (we can't easily call [super buildMenuWithBuilder:] from raw FFI,
    //  but UIViewController's default impl is a no-op, so skipping is fine.)
    menu::build_menubar_for_builder(builder);
}

/// perryMenuAction: callback — dispatches to menu::dispatch_menu_action.
unsafe extern "C" fn vc_perry_menu_action(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    sender: *mut AnyObject,
) {
    menu::dispatch_menu_action(sender);
}

/// canPerformAction:withSender: — return YES for perryMenuAction: so UIKit routes commands here.
unsafe extern "C" fn vc_can_perform_action(
    _this: *mut AnyObject,
    _sel: *const std::ffi::c_void,
    action: *const std::ffi::c_void,
    _sender: *mut AnyObject,
) -> bool {
    let perry_sel = sel_registerName(c"perryMenuAction:".as_ptr());
    action == perry_sel
}

/// Register the PerryViewController class dynamically at runtime.
fn register_view_controller() {
    unsafe {
        let superclass = objc_getClass(c"UIViewController".as_ptr());
        let cls = objc_allocateClassPair(superclass, c"PerryViewController".as_ptr(), 0);
        if cls.is_null() {
            // Already registered
            return;
        }

        // buildMenuWithBuilder: — type encoding: v@:@ (void, self, _cmd, builder)
        let sel_build = sel_registerName(c"buildMenuWithBuilder:".as_ptr());
        class_addMethod(
            cls,
            sel_build,
            vc_build_menu as *const std::ffi::c_void,
            c"v@:@".as_ptr(),
        );

        // perryMenuAction: — type encoding: v@:@ (void, self, _cmd, sender)
        let sel_action = sel_registerName(c"perryMenuAction:".as_ptr());
        class_addMethod(
            cls,
            sel_action,
            vc_perry_menu_action as *const std::ffi::c_void,
            c"v@:@".as_ptr(),
        );

        // canPerformAction:withSender: — type encoding: B@::@ (BOOL, self, _cmd, action, sender)
        let sel_can = sel_registerName(c"canPerformAction:withSender:".as_ptr());
        class_addMethod(
            cls,
            sel_can,
            vc_can_perform_action as *const std::ffi::c_void,
            c"B@::@".as_ptr(),
        );

        objc_registerClassPair(cls);
    }
}

/// Run the iOS app event loop (calls UIApplicationMain, blocks forever).
pub fn app_run(_app_handle: i64) {
    // Force PerryAppDelegate class registration (define_class! registers it lazily)
    let _ = PerryAppDelegate::class();

    // Register PerrySceneDelegate dynamically before UIApplicationMain
    register_scene_delegate();

    // Register PerryViewController (UIViewController + menu bar support)
    register_view_controller();

    unsafe {
        let argc = 0i32;
        let argv: *const *const u8 = std::ptr::null();
        let principal = std::ptr::null::<NSString>();
        let delegate_class_name = NSString::from_str("PerryAppDelegate");

        // UIApplicationMain(argc, argv, nil, @"PerryAppDelegate")
        extern "C" {
            fn UIApplicationMain(
                argc: i32,
                argv: *const *const u8,
                principalClassName: *const NSString,
                delegateClassName: *const NSString,
            ) -> i32;
        }

        UIApplicationMain(argc, argv, principal, &*delegate_class_name);
    }
}

/// Set minimum window size (no-op on iOS — windows are always full-screen).
pub fn set_min_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on iOS
}

/// Set maximum window size (no-op on iOS — windows are always full-screen).
pub fn set_max_size(_app_handle: i64, _w: f64, _h: f64) {
    // No-op on iOS
}

/// Add a keyboard shortcut (stub on iOS — UIKeyCommand not yet implemented).
pub fn add_keyboard_shortcut(_key_ptr: *const u8, _modifiers: f64, _callback: f64) {
    // No-op on iOS for now
}

// ============================================
// Timer
// ============================================

thread_local! {
    static TIMER_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_stdlib_process_pending();
    fn js_promise_run_microtasks() -> i32;
    fn js_nanbox_get_pointer(value: f64) -> i64;
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
}

// ============================================
// Timer Pump — drives setTimeout/setInterval callbacks
// ============================================

pub struct PerryPumpTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPumpTarget"]
    #[ivars = PerryPumpTargetIvars]
    pub struct PerryPumpTarget;

    impl PerryPumpTarget {
        #[unsafe(method(pump:))]
        fn pump(&self, _sender: &AnyObject) {
            unsafe {
                js_callback_timer_tick();
                js_interval_timer_tick();
                js_promise_run_microtasks();
            }
        }
    }
);

impl PerryPumpTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPumpTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

pub struct PerryTimerTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryTimerTarget"]
    #[ivars = PerryTimerTargetIvars]
    pub struct PerryTimerTarget;

    impl PerryTimerTarget {
        #[unsafe(method(timerFired:))]
        fn timer_fired(&self, _sender: &AnyObject) {
            // Drain resolved promises, then run microtasks (.then callbacks)
            unsafe {
                js_stdlib_process_pending();
                js_promise_run_microtasks();
            }

            let key = self.ivars().callback_key.get();
            let closure_f64 = TIMER_CALLBACKS.with(|cbs| {
                cbs.borrow().get(&key).copied()
            });
            if let Some(closure_f64) = closure_f64 {
                let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                unsafe {
                    js_closure_call0(closure_ptr as *const u8);
                }
            }
        }
    }
);

impl PerryTimerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTimerTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set a recurring timer. interval_ms is in milliseconds.
/// The timer calls js_stdlib_process_pending() then invokes the callback.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let interval_secs = interval_ms / 1000.0;

    unsafe {
        let target = PerryTimerTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        TIMER_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, callback);
        });

        // NSTimer.scheduledTimerWithTimeInterval:target:selector:userInfo:repeats:
        let sel = Sel::register(c"timerFired:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: interval_secs,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];

        // Keep the target alive
        std::mem::forget(target);
    }
}
