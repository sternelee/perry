use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Sel};
use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadOnly};
use objc2_app_kit::{NSMenu, NSMenuItem};
use objc2_foundation::{MainThreadMarker, NSObject, NSString};
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    static MENUS: RefCell<Vec<Retained<NSMenu>>> = RefCell::new(Vec::new());
    static MENU_ITEM_CALLBACKS: RefCell<HashMap<usize, f64>> = RefCell::new(HashMap::new());
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
        let header = ptr as *const perry_runtime::string::StringHeader;
        let len = (*header).length as usize;
        let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
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

/// Set a context menu on a widget. Right-click will show this menu.
pub fn set_context_menu(widget_handle: i64, menu_handle: i64) {
    if let (Some(view), Some(menu)) = (crate::widgets::get_widget(widget_handle), get_menu(menu_handle)) {
        unsafe {
            let _: () = msg_send![&*view, setMenu: &*menu];
        }
    }
}
