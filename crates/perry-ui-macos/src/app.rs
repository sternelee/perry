use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventModifierFlags,
    NSImage, NSLayoutConstraint, NSMenu, NSMenuItem, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGSize, CGRect};
use objc2_foundation::{NSObject, NSString, MainThreadMarker};

use std::cell::RefCell;
use std::collections::HashMap;

use crate::widgets;

thread_local! {
    static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    /// Buffered keyboard shortcuts registered before the menu bar exists.
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = RefCell::new(Vec::new());
}

struct PendingShortcut {
    key_ptr: *const u8,
    modifiers: f64,
    callback: f64,
}

thread_local! {
    static ON_TERMINATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static ON_ACTIVATE_CALLBACK: RefCell<Option<f64>> = RefCell::new(None);
    static WINDOWS: RefCell<Vec<WindowEntry>> = RefCell::new(Vec::new());
    static PENDING_ICON_PATH: RefCell<Option<String>> = RefCell::new(None);
    /// Files requested to be opened via macOS Open With / double-click.
    static PENDING_OPEN_FILES: RefCell<Vec<String>> = RefCell::new(Vec::new());
}

struct WindowEntry {
    window: Retained<NSWindow>,
}

struct AppEntry {
    window: Retained<NSWindow>,
    _root_widget: Option<i64>,
}

/// Extract a &str from a *const StringHeader pointer.
fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() {
        return "";
    }
    unsafe {
        let header = ptr as *const crate::string_header::StringHeader;
        let len = (*header).length as usize;
        let data = ptr.add(std::mem::size_of::<crate::string_header::StringHeader>());
        std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
    }
}

/// Create an app with title, width, height.
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() {
        "Perry App"
    } else {
        str_from_header(title_ptr)
    };

    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let frame = CGRect::new(CGPoint::new(200.0, 200.0), CGSize::new(w, h));

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );

        let ns_title = NSString::from_str(title);
        window.setTitle(&ns_title);

        // Match window appearance to the system setting so native controls
        // (NSTextField, NSPopUpButton, etc.) use the correct light/dark theme.
        // isDarkMode() is called here (after NSApp exists) to get the correct value.
        let is_dark = super::perry_system_is_dark_mode() != 0;
        let appearance_name = if is_dark {
            NSString::from_str("NSAppearanceNameDarkAqua")
        } else {
            NSString::from_str("NSAppearanceNameAqua")
        };
        let appearance_cls = objc2::runtime::AnyClass::get(c"NSAppearance").unwrap();
        let appearance: *mut objc2::runtime::AnyObject = objc2::msg_send![
            appearance_cls, appearanceNamed: &*appearance_name
        ];
        if !appearance.is_null() {
            let _: () = objc2::msg_send![&*window, setAppearance: appearance];
        }

        APPS.with(|a| {
            let mut apps = a.borrow_mut();
            apps.push(AppEntry {
                window,
                _root_widget: None,
            });
            apps.len() as i64 // 1-based handle
        })
    }
}

/// Set the root widget (body) of the app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|a| {
        let mut apps = a.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx]._root_widget = Some(root_handle);

            if let Some(view) = widgets::get_widget(root_handle) {
                apps[idx].window.setContentView(Some(&view));

                // Pin the body view to the window's contentLayoutGuide using Auto Layout.
                // contentLayoutGuide accounts for the title bar, so content starts below it.
                unsafe {
                    let _: () = objc2::msg_send![&*view, setTranslatesAutoresizingMaskIntoConstraints: false];
                    let window = &apps[idx].window;
                    let guide: Retained<AnyObject> = msg_send![window, contentLayoutGuide];
                    let guide_top: Retained<AnyObject> = msg_send![&*guide, topAnchor];
                    let guide_bottom: Retained<AnyObject> = msg_send![&*guide, bottomAnchor];
                    let guide_leading: Retained<AnyObject> = msg_send![&*guide, leadingAnchor];
                    let guide_trailing: Retained<AnyObject> = msg_send![&*guide, trailingAnchor];

                    let top_anchor = view.topAnchor();
                    let bottom_anchor = view.bottomAnchor();
                    let leading_anchor = view.leadingAnchor();
                    let trailing_anchor = view.trailingAnchor();

                    let c_top: Retained<AnyObject> = msg_send![&*top_anchor, constraintEqualToAnchor: &*guide_top];
                    let c_bottom: Retained<AnyObject> = msg_send![&*bottom_anchor, constraintEqualToAnchor: &*guide_bottom];
                    let c_leading: Retained<AnyObject> = msg_send![&*leading_anchor, constraintEqualToAnchor: &*guide_leading];
                    let c_trailing: Retained<AnyObject> = msg_send![&*trailing_anchor, constraintEqualToAnchor: &*guide_trailing];

                    let _: () = msg_send![&*c_top, setActive: true];
                    let _: () = msg_send![&*c_bottom, setActive: true];
                    let _: () = msg_send![&*c_leading, setActive: true];
                    let _: () = msg_send![&*c_trailing, setActive: true];
                }
            }
        }
    });
}

/// Create a standard macOS menu bar with Quit (Cmd+Q) support,
/// OR install the user-provided menu bar from menuBarAttach().
fn setup_menu_bar(app: &NSApplication, mtm: MainThreadMarker) {
    // Check if the user already attached a custom menu bar via menuBarAttach()
    let user_bar = crate::menu::PENDING_USER_MENUBAR.with(|p| p.borrow_mut().take());

    let has_user_bar = user_bar.is_some();

    unsafe {
        let menu_bar = if let Some(bar) = user_bar {
            // User provided a menu bar — prepend the app menu (with Quit) as the first item
            let app_menu_item = NSMenuItem::new(mtm);
            let app_menu = NSMenu::new(mtm);

            let quit_title = NSString::from_str("Quit");
            let quit_key = NSString::from_str("q");
            let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &quit_title,
                Some(Sel::register(c"terminate:")),
                &quit_key,
            );
            quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            app_menu.addItem(&quit_item);
            app_menu_item.setSubmenu(Some(&app_menu));

            // Insert app menu at position 0 (before user menus)
            bar.insertItem_atIndex(&app_menu_item, 0);
            bar
        } else {
            // No user menu bar — create default with just Quit
            let menu_bar = NSMenu::new(mtm);

            let app_menu_item = NSMenuItem::new(mtm);
            let app_menu = NSMenu::new(mtm);

            let quit_title = NSString::from_str("Quit");
            let quit_key = NSString::from_str("q");
            let quit_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &quit_title,
                Some(Sel::register(c"terminate:")),
                &quit_key,
            );
            quit_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            app_menu.addItem(&quit_item);

            app_menu_item.setSubmenu(Some(&app_menu));
            menu_bar.addItem(&app_menu_item);
            menu_bar
        };

        // Only add the default Edit menu when the user hasn't provided a custom menu bar.
        // User-provided menus should include their own Edit menu with responder-chain actions.
        if !has_user_bar {
            let edit_menu_item = NSMenuItem::new(mtm);
            let edit_menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("Edit"));

            let undo_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Undo"),
                Some(Sel::register(c"undo:")), &NSString::from_str("z"));
            undo_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&undo_item);

            let redo_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Redo"),
                Some(Sel::register(c"redo:")), &NSString::from_str("z"));
            redo_item.setKeyEquivalentModifierMask(
                NSEventModifierFlags::Command | NSEventModifierFlags::Shift);
            edit_menu.addItem(&redo_item);

            edit_menu.addItem(&NSMenuItem::separatorItem(mtm));

            let cut_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Cut"),
                Some(Sel::register(c"cut:")), &NSString::from_str("x"));
            cut_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&cut_item);

            let copy_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Copy"),
                Some(Sel::register(c"copy:")), &NSString::from_str("c"));
            copy_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&copy_item);

            let paste_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Paste"),
                Some(Sel::register(c"paste:")), &NSString::from_str("v"));
            paste_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&paste_item);

            let select_all_item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm), &NSString::from_str("Select All"),
                Some(Sel::register(c"selectAll:")), &NSString::from_str("a"));
            select_all_item.setKeyEquivalentModifierMask(NSEventModifierFlags::Command);
            edit_menu.addItem(&select_all_item);

            edit_menu_item.setSubmenu(Some(&edit_menu));
            menu_bar.addItem(&edit_menu_item);
        }

        app.setMainMenu(Some(&menu_bar));
    }
}

/// Run the application event loop (blocks).
pub fn app_run(_app_handle: i64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    // Apply pending dock icon
    PENDING_ICON_PATH.with(|p| {
        if let Some(path) = p.borrow().as_ref() {
            unsafe {
                let ns_path = NSString::from_str(path);
                let image: Option<Retained<NSImage>> = msg_send![
                    NSImage::alloc(), initWithContentsOfFile: &*ns_path
                ];
                if let Some(img) = image {
                    let _: () = msg_send![&*app, setApplicationIconImage: &*img];
                }
            }
        }
    });

    // Set up menu bar with Cmd+Q support
    setup_menu_bar(&app, mtm);

    // Install any keyboard shortcuts that were registered before the menu existed
    flush_pending_shortcuts(mtm);

    APPS.with(|a| {
        let apps = a.borrow();
        for entry in apps.iter() {
            entry.window.center();

            // Validate window is on a visible screen — if the position was
            // restored from a previous session with a different display setup,
            // the window could be completely off-screen.
            unsafe {
                let frame: CGRect = msg_send![&*entry.window, frame];
                let screens: Retained<AnyObject> = msg_send![
                    objc2::class!(NSScreen), screens
                ];
                let screen_count: usize = msg_send![&*screens, count];

                let mut on_screen = false;
                for i in 0..screen_count {
                    let screen: *const AnyObject = msg_send![&*screens, objectAtIndex: i];
                    let visible_frame: CGRect = msg_send![screen, visibleFrame];
                    if frame.origin.x >= visible_frame.origin.x - frame.size.width * 0.5
                        && frame.origin.x < visible_frame.origin.x + visible_frame.size.width
                        && frame.origin.y >= visible_frame.origin.y - frame.size.height * 0.5
                        && frame.origin.y < visible_frame.origin.y + visible_frame.size.height
                    {
                        on_screen = true;
                        break;
                    }
                }

                if !on_screen {
                    entry.window.center();
                }
            }

            entry.window.makeKeyAndOrderFront(None);
        }
    });

    // Set up app delegate for file open events
    unsafe {
        let delegate = PerryAppDelegate::new();
        let _: () = msg_send![&*app, setDelegate: &*delegate];
        std::mem::forget(delegate);
    }

    // Also check process argv for files passed on command line
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 {
        PENDING_OPEN_FILES.with(|files| {
            let mut files = files.borrow_mut();
            for arg in args.iter().skip(1) {
                if !arg.starts_with('-') {
                    files.push(arg.clone());
                }
            }
        });
    }

    // Activate the app (bring to front)
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    // Set up a recurring timer pump to drive setTimeout/setInterval callbacks.
    // Without this, TypeScript timer-based loops (event polling, animation) never fire.
    // ~8ms interval (120Hz) ensures responsive callback delivery.
    unsafe {
        let target = PerryPumpTarget::new();
        let sel = Sel::register(c"pump:");
        let _: Retained<AnyObject> = msg_send![
            objc2::class!(NSTimer),
            scheduledTimerWithTimeInterval: 0.008f64,
            target: &*target,
            selector: sel,
            userInfo: std::ptr::null::<AnyObject>(),
            repeats: true
        ];
        // Keep the target alive for the duration of the app
        std::mem::forget(target);
    }

    app.run();
}

/// Set the minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window.setMinSize(CGSize::new(w, h));
        }
    });
}

/// Set the maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|a| {
        let apps = a.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].window.setMaxSize(CGSize::new(w, h));
        }
    });
}

// ============================================
// Keyboard Shortcuts
// ============================================

thread_local! {
    static SHORTCUT_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryShortcutTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryShortcutTarget"]
    #[ivars = PerryShortcutTargetIvars]
    pub struct PerryShortcutTarget;

    impl PerryShortcutTarget {
        #[unsafe(method(shortcutFired:))]
        fn shortcut_fired(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("shortcut callback", std::panic::AssertUnwindSafe(|| {
                let key = self.ivars().callback_key.get();
                let closure_f64 = SHORTCUT_CALLBACKS.with(|cbs| {
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

impl PerryShortcutTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryShortcutTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

/// Set the application dock icon from a file path.
/// Stores the path; the icon is applied in app_run after activation policy is set.
pub fn app_set_icon(path_ptr: *const u8) {
    let path = str_from_header(path_ptr);
    if !path.is_empty() {
        PENDING_ICON_PATH.with(|p| {
            *p.borrow_mut() = Some(path.to_string());
        });
    }
}

/// Add a keyboard shortcut to the app menu.
/// `key_ptr` is a StringHeader pointer to the key character (e.g., "s" for Cmd+S).
/// `modifiers` is a bitfield: 1=Cmd, 2=Shift, 4=Option, 8=Control.
/// `callback` is a NaN-boxed closure pointer.
///
/// If the menu bar doesn't exist yet (called before `app_run`), the shortcut
/// is buffered and will be installed once the menu is created.
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let app = NSApplication::sharedApplication(mtm);

    // If the menu bar exists, install immediately; otherwise buffer for later.
    let has_menu = unsafe { app.mainMenu().is_some() };
    if has_menu {
        install_keyboard_shortcut(key_ptr, modifiers, callback, mtm);
    } else {
        PENDING_SHORTCUTS.with(|ps| {
            ps.borrow_mut().push(PendingShortcut { key_ptr, modifiers, callback });
        });
    }
}

/// Install a single keyboard shortcut into the existing app menu.
fn install_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64, mtm: MainThreadMarker) {
    let key_str = str_from_header(key_ptr);
    let app = NSApplication::sharedApplication(mtm);

    unsafe {
        let target = PerryShortcutTarget::new();
        let target_addr = Retained::as_ptr(&target) as usize;
        target.ivars().callback_key.set(target_addr);

        SHORTCUT_CALLBACKS.with(|cbs| {
            cbs.borrow_mut().insert(target_addr, callback);
        });

        let ns_key = NSString::from_str(key_str);
        let title = NSString::from_str(&format!("Shortcut {}", key_str));
        let item = NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &title,
            Some(Sel::register(c"shortcutFired:")),
            &ns_key,
        );

        // Build modifier flags
        let mod_bits = modifiers as u64;
        let mut flags = NSEventModifierFlags::empty();
        if mod_bits & 1 != 0 { flags |= NSEventModifierFlags::Command; }
        if mod_bits & 2 != 0 { flags |= NSEventModifierFlags::Shift; }
        if mod_bits & 4 != 0 { flags |= NSEventModifierFlags::Option; }
        if mod_bits & 8 != 0 { flags |= NSEventModifierFlags::Control; }
        item.setKeyEquivalentModifierMask(flags);

        item.setTarget(Some(&target));
        std::mem::forget(target);

        // Add to the app menu (first menu item's submenu)
        if let Some(main_menu) = app.mainMenu() {
            if main_menu.numberOfItems() > 0 {
                if let Some(app_menu_item) = main_menu.itemAtIndex(0) {
                    if let Some(submenu) = app_menu_item.submenu() {
                        submenu.addItem(&item);
                    }
                }
            }
        }
    }
}

/// Flush any keyboard shortcuts that were registered before the menu bar existed.
fn flush_pending_shortcuts(mtm: MainThreadMarker) {
    flush_pending_shortcuts_inner(mtm);
}

/// Public entry point for flushing pending shortcuts (called from menu::menubar_attach).
pub fn flush_pending_shortcuts_pub(mtm: MainThreadMarker) {
    flush_pending_shortcuts_inner(mtm);
}

fn flush_pending_shortcuts_inner(mtm: MainThreadMarker) {
    PENDING_SHORTCUTS.with(|ps| {
        let pending: Vec<PendingShortcut> = ps.borrow_mut().drain(..).collect();
        for shortcut in pending {
            install_keyboard_shortcut(shortcut.key_ptr, shortcut.modifiers, shortcut.callback, mtm);
        }
    });
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
    fn js_callback_timer_tick() -> i32;
    fn js_interval_timer_tick() -> i32;
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
            crate::catch_callback_panic("timer callback", std::panic::AssertUnwindSafe(|| {
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
            }));
        }
    }
);

// ============================================
// Timer Pump — drives setTimeout/setInterval callbacks
// ============================================

// Separate target class for the run loop pump timer.
// Only calls timer tick functions from perry-runtime (always linked).
// Does NOT call js_stdlib_process_pending (which may not be linked in
// pure UI apps that don't use --enable-js-runtime).
pub struct PerryPumpTargetIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryPumpTarget"]
    #[ivars = PerryPumpTargetIvars]
    pub struct PerryPumpTarget;

    impl PerryPumpTarget {
        #[unsafe(method(pump:))]
        fn pump(&self, _sender: &AnyObject) {
            crate::catch_callback_panic("pump", std::panic::AssertUnwindSafe(|| {
                unsafe {
                    js_callback_timer_tick();
                    js_interval_timer_tick();
                    js_promise_run_microtasks();
                }
            }));
        }
    }
);

impl PerryPumpTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryPumpTargetIvars);
        unsafe { msg_send![super(this), init] }
    }
}

impl PerryTimerTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryTimerTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
}

// ============================================
// Application Delegate — handles file open events
// ============================================

pub struct PerryAppDelegateIvars;

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryAppDelegate"]
    #[ivars = PerryAppDelegateIvars]
    pub struct PerryAppDelegate;

    impl PerryAppDelegate {
        #[unsafe(method(application:openFile:))]
        fn application_open_file(&self, _app: &AnyObject, filename: &NSString) -> bool {
            let path = filename.to_string();
            PENDING_OPEN_FILES.with(|files| {
                files.borrow_mut().push(path);
            });
            true
        }
    }
);

impl PerryAppDelegate {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryAppDelegateIvars);
        unsafe { msg_send![super(this), init] }
    }
}

/// Poll for pending open-file requests. Returns the next file path or empty string.
pub fn poll_open_file() -> String {
    PENDING_OPEN_FILES.with(|files| {
        let mut files = files.borrow_mut();
        if files.is_empty() {
            String::new()
        } else {
            files.remove(0)
        }
    })
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

// ============================================
// Lifecycle Hooks
// ============================================

/// Register an onTerminate callback.
pub fn register_on_terminate(callback: f64) {
    ON_TERMINATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

/// Register an onActivate callback.
pub fn register_on_activate(callback: f64) {
    ON_ACTIVATE_CALLBACK.with(|cb| {
        *cb.borrow_mut() = Some(callback);
    });
}

// ============================================
// Multi-Window
// ============================================

/// Create a new window. Returns 1-based handle.
pub fn window_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = if title_ptr.is_null() { "Window" } else { str_from_header(title_ptr) };
    let w = if width > 0.0 { width } else { 400.0 };
    let h = if height > 0.0 { height } else { 300.0 };

    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        let frame = CGRect::new(CGPoint::new(200.0, 200.0), CGSize::new(w, h));

        let window = NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            frame,
            style,
            NSBackingStoreType::Buffered,
            false,
        );

        let ns_title = NSString::from_str(title);
        window.setTitle(&ns_title);

        WINDOWS.with(|w| {
            let mut windows = w.borrow_mut();
            windows.push(WindowEntry { window });
            windows.len() as i64
        })
    }
}

/// Set the root widget of a window.
pub fn window_set_body(window_handle: i64, widget_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            if let Some(view) = crate::widgets::get_widget(widget_handle) {
                windows[idx].window.setContentView(Some(&view));
            }
        }
    });
}

/// Show a window (make key and order front).
pub fn window_show(window_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            windows[idx].window.center();
            windows[idx].window.makeKeyAndOrderFront(None);
        }
    });
}

/// Close a window.
pub fn window_close(window_handle: i64) {
    WINDOWS.with(|w| {
        let windows = w.borrow();
        let idx = (window_handle - 1) as usize;
        if idx < windows.len() {
            windows[idx].window.close();
        }
    });
}
