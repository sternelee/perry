use objc2::rc::Retained;
use objc2::msg_send;
use objc2::{AnyThread, MainThreadOnly};
use objc2_app_kit::{NSImage, NSImageView, NSView};
use objc2_foundation::{MainThreadMarker, NSString};

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

/// Create an NSImageView displaying an SF Symbol by name. Returns widget handle.
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    unsafe {
        let ns_name = NSString::from_str(name);
        let image: Option<Retained<NSImage>> = msg_send![
            objc2::runtime::AnyClass::get(c"NSImage").unwrap(),
            imageWithSystemSymbolName: &*ns_name,
            accessibilityDescription: std::ptr::null::<NSString>()
        ];

        let image_view: Retained<NSImageView> = msg_send![
            NSImageView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(24.0, 24.0),
            )
        ];

        if let Some(img) = image {
            let _: () = msg_send![&*image_view, setImage: &*img];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        super::register_widget(view)
    }
}

/// Create an NSImageView displaying an image loaded from a file path. Returns widget handle.
pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let mtm = MainThreadMarker::new().expect("perry/ui must run on the main thread");

    // Resolve relative paths against the executable's directory (inside .app bundle)
    let resolved = if !path.starts_with('/') {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                let candidate = exe_dir.join(path);
                if candidate.exists() {
                    candidate.to_string_lossy().to_string()
                } else {
                    path.to_string()
                }
            } else {
                path.to_string()
            }
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    };

    unsafe {
        let ns_path = NSString::from_str(&resolved);
        let image: Option<Retained<NSImage>> = msg_send![
            NSImage::alloc(), initWithContentsOfFile: &*ns_path
        ];

        let image_view: Retained<NSImageView> = msg_send![
            NSImageView::alloc(mtm), initWithFrame: objc2_core_foundation::CGRect::new(
                objc2_core_foundation::CGPoint::new(0.0, 0.0),
                objc2_core_foundation::CGSize::new(64.0, 64.0),
            )
        ];

        // Scale image to fit the view frame (prevents intrinsic size from overriding)
        let _: () = msg_send![&*image_view, setImageScaling: 2_isize]; // NSImageScaleProportionallyUpOrDown

        if let Some(img) = image {
            let _: () = msg_send![&*image_view, setImage: &*img];
        }

        let view: Retained<NSView> = Retained::cast_unchecked(image_view);
        super::register_widget(view)
    }
}

/// Set the frame size of an image widget.
pub fn set_size(handle: i64, width: f64, height: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            // Resize the NSImage itself so intrinsic content size matches
            let image: *mut objc2::runtime::AnyObject = msg_send![&*view, image];
            if !image.is_null() {
                let img_size = objc2_core_foundation::CGSize::new(width, height);
                let _: () = msg_send![image, setSize: img_size];
            }
            let size = objc2_core_foundation::CGSize::new(width, height);
            let _: () = msg_send![&*view, setFrameSize: size];
        }
    }
}

/// Set the content tint color of an image widget (for SF Symbols).
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view) = super::get_widget(handle) {
        unsafe {
            let color: *mut objc2::runtime::AnyObject = msg_send![
                objc2::runtime::AnyClass::get(c"NSColor").unwrap(),
                colorWithRed: r, green: g, blue: b, alpha: a
            ];
            let _: () = msg_send![&*view, setContentTintColor: color];
        }
    }
}
