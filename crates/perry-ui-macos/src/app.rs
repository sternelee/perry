use objc2::rc::Retained;
use objc2::runtime::Sel;
use objc2::MainThreadOnly;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventModifierFlags,
    NSLayoutConstraint, NSMenu, NSMenuItem, NSWindow, NSWindowStyleMask,
};
use objc2_core_foundation::{CGPoint, CGSize, CGRect};
use objc2_foundation::{NSString, MainThreadMarker};

use std::cell::RefCell;

use crate::widgets;

thread_local! {
    static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
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
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).length as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
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
                unsafe {
                    // Disable autoresizing mask translation for Auto Layout
                    view.setTranslatesAutoresizingMaskIntoConstraints(false);
                }

                apps[idx].window.setContentView(Some(&view));

                // Pin the root widget to fill the window's content view
                if let Some(content_view) = apps[idx].window.contentView() {
                    unsafe {
                        let leading = view.leadingAnchor();
                        let trailing = view.trailingAnchor();
                        let top = view.topAnchor();
                        let bottom = view.bottomAnchor();
                        let cv_leading = content_view.leadingAnchor();
                        let cv_trailing = content_view.trailingAnchor();
                        let cv_top = content_view.topAnchor();
                        let cv_bottom = content_view.bottomAnchor();
                        NSLayoutConstraint::activateConstraints(&objc2_foundation::NSArray::from_retained_slice(&[
                            leading.constraintEqualToAnchor(&cv_leading),
                            trailing.constraintEqualToAnchor(&cv_trailing),
                            top.constraintEqualToAnchor(&cv_top),
                            bottom.constraintEqualToAnchor(&cv_bottom),
                        ]));
                    }
                }
            }
        }
    });
}

/// Create a standard macOS menu bar with Quit (Cmd+Q) support.
fn setup_menu_bar(app: &NSApplication, mtm: MainThreadMarker) {
    unsafe {
        // Main menu bar
        let menu_bar = NSMenu::new(mtm);

        // App menu (the first menu, shown as the app name)
        let app_menu_item = NSMenuItem::new(mtm);
        let app_menu = NSMenu::new(mtm);

        // Quit menu item: "Quit" with Cmd+Q
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

        app.setMainMenu(Some(&menu_bar));
    }
}

/// Run the application event loop (blocks).
pub fn app_run(_app_handle: i64) {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

    // Set up menu bar with Cmd+Q support
    setup_menu_bar(&app, mtm);

    APPS.with(|a| {
        let apps = a.borrow();
        for entry in apps.iter() {
            entry.window.center();
            entry.window.makeKeyAndOrderFront(None);
        }
    });

    // Activate the app (bring to front)
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);

    app.run();
}
