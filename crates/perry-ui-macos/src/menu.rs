use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static MENUS: RefCell<Vec<Retained<NSMenu>>> = RefCell::new(Vec::new());
    static MENU_ITEM_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
    static MENUBARS: RefCell<Vec<Retained<NSMenu>>> = RefCell::new(Vec::new());
    /// Pending user menu bar to install during app_run (set by menubar_attach).
    pub(crate) static PENDING_USER_MENUBAR: RefCell<Option<Retained<NSMenu>>> = RefCell::new(None);
}

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

pub struct PerryMenuItemTargetIvars {
    callback_key: std::cell::Cell<usize>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "PerryMenuItemTarget"]
    #[ivars = PerryMenuItemTargetIvars]
    pub struct PerryMenuItemTarget;

    impl PerryMenuItemTarget {
        #[unsafe(method(menuItemClicked:))]
        fn menu_item_clicked(&self, _sender: &AnyObject) {
            let key = self.ivars().callback_key.get();
            MENU_ITEM_CALLBACKS.with(|cbs| {
                if let Some(&closure_f64) = cbs.borrow().get(&key) {
                    let closure_ptr = unsafe { js_nanbox_get_pointer(closure_f64) };
                    unsafe {
                        js_closure_call0(closure_ptr as *const u8);
                    }
                }
            });
        }
    }
);

impl PerryMenuItemTarget {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(PerryMenuItemTargetIvars {
            callback_key: std::cell::Cell::new(0),
        });
        unsafe { msg_send![super(this), init] }
    }
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

/// Create a new context menu. Returns menu handle (1-based).
pub fn create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let menu = NSMenu::new(mtm);
    MENUS.with(|m| {
        let mut menus = m.borrow_mut();
        menus.push(menu);
        menus.len() as i64
    })
}

/// Get a menu by handle.
fn get_menu(handle: i64) -> Option<Retained<NSMenu>> {
    MENUS.with(|m| {
        let menus = m.borrow();
        let idx = (handle - 1) as usize;
        menus.get(idx).cloned()
    })
}

/// Add an item to a menu with a title and callback.
pub fn add_item(menu_handle: i64, title_ptr: *const u8, callback: f64) {
    let title = str_from_header(title_ptr);
    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                Some(Sel::register(c"menuItemClicked:")),
                &empty_key,
            );

            let target = PerryMenuItemTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            MENU_ITEM_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            item.setTarget(Some(&target));
            std::mem::forget(target);

            menu.addItem(&item);
        }
    }
}

/// Add an item to a menu with a title, callback, and keyboard shortcut.
pub fn add_item_with_shortcut(menu_handle: i64, title_ptr: *const u8, callback: f64, shortcut_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let shortcut_str = str_from_header(shortcut_ptr);
    let (key, flags) = parse_shortcut(shortcut_str);

    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let ns_key = NSString::from_str(&key);
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                Some(Sel::register(c"menuItemClicked:")),
                &ns_key,
            );
            item.setKeyEquivalentModifierMask(flags);

            let target = PerryMenuItemTarget::new();
            let target_addr = Retained::as_ptr(&target) as usize;
            target.ivars().callback_key.set(target_addr);

            MENU_ITEM_CALLBACKS.with(|cbs| {
                cbs.borrow_mut().insert(target_addr, callback);
            });

            item.setTarget(Some(&target));
            std::mem::forget(target);

            menu.addItem(&item);
        }
    }
}

/// Add a separator item to a menu.
pub fn add_separator(menu_handle: i64) {
    if let Some(menu) = get_menu(menu_handle) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let sep = NSMenuItem::separatorItem(mtm);
        menu.addItem(&sep);
    }
}

/// Add a submenu to a menu.
pub fn add_submenu(menu_handle: i64, title_ptr: *const u8, submenu_handle: i64) {
    let title = str_from_header(title_ptr);
    if let (Some(menu), Some(submenu)) = (get_menu(menu_handle), get_menu(submenu_handle)) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                None,
                &empty_key,
            );
            // Set the submenu's title to match
            submenu.setTitle(&ns_title);
            item.setSubmenu(Some(&submenu));
            menu.addItem(&item);
        }
    }
}

/// Create a new menu bar. Returns bar handle (1-based).
pub fn menubar_create() -> i64 {
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
    let menu = NSMenu::new(mtm);
    MENUBARS.with(|m| {
        let mut bars = m.borrow_mut();
        bars.push(menu);
        bars.len() as i64
    })
}

/// Get a menu bar by handle.
fn get_menubar(handle: i64) -> Option<Retained<NSMenu>> {
    MENUBARS.with(|m| {
        let bars = m.borrow();
        let idx = (handle - 1) as usize;
        bars.get(idx).cloned()
    })
}

/// Add a menu to the menu bar with a title.
pub fn menubar_add_menu(bar_handle: i64, title_ptr: *const u8, menu_handle: i64) {
    let title = str_from_header(title_ptr);
    if let (Some(bar), Some(menu)) = (get_menubar(bar_handle), get_menu(menu_handle)) {
        let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");
        let ns_title = NSString::from_str(title);
        let empty_key = NSString::from_str("");
        unsafe {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &ns_title,
                None,
                &empty_key,
            );
            // Set the menu's title so it displays correctly in the bar
            menu.setTitle(&ns_title);
            item.setSubmenu(Some(&menu));
            bar.addItem(&item);
        }
    }
}

/// Attach a menu bar to the application, replacing the default menu bar.
/// Stores bar as pending so app_run() can install it instead of the default.
pub fn menubar_attach(bar_handle: i64) {
    eprintln!("[perry/ui] menubar_attach called with handle {}", bar_handle);
    if let Some(bar) = get_menubar(bar_handle) {
        let count: usize = unsafe { objc2::msg_send![&*bar, numberOfItems] };
        eprintln!("[perry/ui] menubar_attach: bar has {} items, storing as pending", count);
        PENDING_USER_MENUBAR.with(|p| {
            *p.borrow_mut() = Some(bar);
        });
    } else {
        eprintln!("[perry/ui] menubar_attach: bar handle {} not found!", bar_handle);
    }
}

/// Set a context menu on a widget. Right-click will show this menu.
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    if let (Some(view), Some(menu)) = (crate::widgets::get_widget(widget_handle), get_menu(menu_handle)) {
        unsafe {
            let _: () = msg_send![&*view, setMenu: &*menu];
        }
    }
}

/// Parse a shortcut string like "Cmd+Shift+N" into (key, NSEventModifierFlags).
fn parse_shortcut(s: &str) -> (String, NSEventModifierFlags) {
    let mut flags = NSEventModifierFlags::empty();
    let parts: Vec<&str> = s.split('+').collect();
    let mut key = String::new();

    for part in &parts {
        let trimmed = part.trim();
        match trimmed.to_lowercase().as_str() {
            "cmd" | "command" => flags |= NSEventModifierFlags::Command,
            "shift" => flags |= NSEventModifierFlags::Shift,
            "option" | "alt" => flags |= NSEventModifierFlags::Option,
            "ctrl" | "control" => flags |= NSEventModifierFlags::Control,
            _ => key = trimmed.to_lowercase(),
        }
    }

    (key, flags)
}
