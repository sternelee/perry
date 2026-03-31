//! Image widget — custom PerryImage window with GDI+ alpha-blended painting
//! for file images, or STATIC+SS_ICON for symbol images.

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::InvalidateRect;
#[cfg(target_os = "windows")]
use windows::Win32::System::SystemServices::{SS_ICON};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use super::{WidgetKind, alloc_control_id, register_widget};

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

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// STM_SETIMAGE message
#[cfg(target_os = "windows")]
const STM_SETIMAGE: u32 = 0x0172;

/// Per-widget tint color (limited use on Win32 — stored for potential custom draw)
struct ImageTint {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

thread_local! {
    static IMAGE_TINTS: RefCell<HashMap<i64, ImageTint>> = RefCell::new(HashMap::new());
    /// Store resolved file paths keyed by widget handle
    static IMAGE_PATHS: RefCell<HashMap<i64, String>> = RefCell::new(HashMap::new());
    /// Map from HWND (as isize) -> resolved file path for WM_PAINT lookup
    #[cfg(target_os = "windows")]
    static HWND_TO_PATH: RefCell<HashMap<isize, String>> = RefCell::new(HashMap::new());
}

/// WM_PAINT handler for PerryImage windows — draws the image with GDI+ alpha blending
/// so PNG transparency composites correctly over the parent's background (gradient or solid).
#[cfg(target_os = "windows")]
unsafe extern "system" fn image_wnd_proc(
    hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            use windows::Win32::Graphics::Gdi::*;
            use windows::Win32::Graphics::GdiPlus::*;

            let path = HWND_TO_PATH.with(|m| m.borrow().get(&(hwnd.0 as isize)).cloned());

            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            if let Some(path) = path {
                let wide_path = to_wide(&path);
                let mut token: usize = 0;
                let input = GdiplusStartupInput { GdiplusVersion: 1, ..Default::default() };
                if GdiplusStartup(&mut token, &input, std::ptr::null_mut()).0 == 0 {
                    let mut gp_image: *mut GpImage = std::ptr::null_mut();
                    let status = GdipLoadImageFromFile(
                        windows::core::PCWSTR(wide_path.as_ptr()), &mut gp_image,
                    );
                    if status.0 == 0 && !gp_image.is_null() {
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let w = rect.right - rect.left;
                        let h = rect.bottom - rect.top;

                        let mut graphics: *mut GpGraphics = std::ptr::null_mut();
                        GdipCreateFromHDC(hdc, &mut graphics);
                        if !graphics.is_null() {
                            GdipSetInterpolationMode(graphics, InterpolationMode(7)); // HighQualityBicubic

                            // Preserve aspect ratio: fit image within the widget rect
                            let mut img_w: u32 = 0;
                            let mut img_h: u32 = 0;
                            GdipGetImageWidth(gp_image, &mut img_w);
                            GdipGetImageHeight(gp_image, &mut img_h);
                            let (dx, dy, dw, dh) = if img_w > 0 && img_h > 0 {
                                let src_r = img_w as f64 / img_h as f64;
                                let dst_r = w as f64 / h as f64;
                                if src_r > dst_r {
                                    let fh = (w as f64 / src_r) as i32;
                                    (0, (h - fh) / 2, w, fh)
                                } else {
                                    let fw = (h as f64 * src_r) as i32;
                                    ((w - fw) / 2, 0, fw, h)
                                }
                            } else {
                                (0, 0, w, h)
                            };
                            GdipDrawImageRectI(graphics, gp_image, dx, dy, dw, dh);
                            GdipDeleteGraphics(graphics);
                        }
                        GdipDisposeImage(gp_image);
                    }
                    GdiplusShutdown(token);
                }
            }

            EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_ERASEBKGND => {
            // Don't erase — parent's background (gradient) already painted, and
            // GDI+ alpha blending in WM_PAINT composites on top of it.
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Register the PerryImage window class (idempotent — safe to call multiple times).
#[cfg(target_os = "windows")]
fn ensure_image_class_registered() {
    use std::sync::Once;
    static REGISTERED: Once = Once::new();
    REGISTERED.call_once(|| unsafe {
        let hinstance = GetModuleHandleW(None).unwrap();
        let class_name = to_wide("PerryImage");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(image_wnd_proc),
            hInstance: hinstance.into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(std::ptr::null_mut()), // transparent
            lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };
        RegisterClassExW(&wc);
    });
}

/// Resolve a relative asset path against the executable's directory first,
/// falling back to the path as-is (relative to cwd). Matches macOS/GTK behavior.
#[cfg(target_os = "windows")]
fn resolve_asset_path(path: &str) -> String {
    if std::path::Path::new(path).is_absolute() {
        return path.to_string();
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(exe_dir) = exe.parent() {
            let candidate = exe_dir.join(path);
            if candidate.exists() {
                return candidate.to_string_lossy().to_string();
            }
        }
    }
    path.to_string()
}

/// Create an Image from a file path. Returns widget handle.
pub fn create_file(path_ptr: *const u8) -> i64 {
    let path = str_from_header(path_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let resolved = resolve_asset_path(path);
        ensure_image_class_registered();

        let class_name = to_wide("PerryImage");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                None,
                WS_CHILD | WS_VISIBLE,
                0, 0, 100, 100,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Store path for WM_PAINT lookup
            HWND_TO_PATH.with(|m| {
                m.borrow_mut().insert(hwnd.0 as isize, resolved.clone());
            });

            let handle = register_widget(hwnd, WidgetKind::Image, control_id);
            IMAGE_PATHS.with(|p| p.borrow_mut().insert(handle, resolved));
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = path;
        register_widget(0, WidgetKind::Image, control_id)
    }
}

/// Create an Image from a system symbol/icon name. Returns widget handle.
pub fn create_symbol(name_ptr: *const u8) -> i64 {
    let name = str_from_header(name_ptr);
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let class_name = to_wide("STATIC");
        let window_text = to_wide("");
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(window_text.as_ptr()),
                WINDOW_STYLE(SS_ICON.0 | WS_CHILD.0 | WS_VISIBLE.0),
                0, 0, 32, 32,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            )
            .unwrap();

            // Map common symbol names to system icons
            let icon_id = match name {
                "exclamationmark.triangle" | "warning" => IDI_WARNING,
                "info.circle" | "info" => IDI_INFORMATION,
                "xmark.circle" | "error" => IDI_ERROR,
                "questionmark.circle" | "question" => IDI_QUESTION,
                "app" | "application" => IDI_APPLICATION,
                "shield" | "shield.fill" => IDI_SHIELD,
                _ => IDI_APPLICATION,
            };

            let hicon = LoadIconW(None, icon_id);
            if let Ok(hicon) = hicon {
                SendMessageW(
                    hwnd,
                    STM_SETIMAGE,
                    WPARAM(IMAGE_ICON.0 as usize),
                    LPARAM(hicon.0 as isize),
                );
            }

            register_widget(hwnd, WidgetKind::Image, control_id)
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
        register_widget(0, WidgetKind::Image, control_id)
    }
}

/// Invalidate the image so it repaints at the current layout size.
/// Called by the layout engine after `MoveWindow` for Image widgets.
#[cfg(target_os = "windows")]
pub fn reload_bitmap_scaled(handle: i64, _w: i32, _h: i32) {
    // With GDI+ alpha-blended WM_PAINT, we just need to invalidate.
    // The paint handler reads the current client rect and draws at that size.
    if let Some(hwnd) = super::get_hwnd(handle) {
        unsafe { let _ = InvalidateRect(hwnd, None, false); }
    }
}

/// Set the size of an Image widget.
pub fn set_size(handle: i64, width: f64, height: f64) {
    // Also set fixed dimensions so the layout engine uses these
    super::set_fixed_width(handle, width as i32);
    super::set_fixed_height(handle, height as i32);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let w = width as i32;
            let h = height as i32;
            unsafe {
                let _ = SetWindowPos(hwnd, None, 0, 0, w, h, SWP_NOMOVE | SWP_NOZORDER);
                let _ = InvalidateRect(hwnd, None, false);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, width, height);
    }
}

/// Set the tint color for an Image widget.
/// On Win32, tinting is limited — we store the color for potential custom-draw use.
pub fn set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    IMAGE_TINTS.with(|tints| {
        tints.borrow_mut().insert(handle, ImageTint {
            r: (r * 255.0) as u8,
            g: (g * 255.0) as u8,
            b: (b * 255.0) as u8,
            a: (a * 255.0) as u8,
        });
    });

    #[cfg(target_os = "windows")]
    {
        // Force repaint (custom-draw could use the tint if implemented)
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = handle;
    }
}
