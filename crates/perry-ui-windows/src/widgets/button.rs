//! Button widget — Win32 BUTTON control (BS_PUSHBUTTON)

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{InvalidateRect, SetTextColor, SetBkMode, TRANSPARENT, DrawTextW, DT_CENTER, DT_VCENTER, DT_SINGLELINE, FillRect, SelectObject, HGDIOBJ};
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;

use super::{WidgetKind, alloc_control_id, register_widget};

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

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

thread_local! {
    // Map from widget handle -> callback pointer
    static BUTTON_CALLBACKS: RefCell<HashMap<i64, *const u8>> = RefCell::new(HashMap::new());
    // Map from widget handle -> text COLORREF
    static BUTTON_TEXT_COLORS: RefCell<HashMap<i64, u32>> = RefCell::new(HashMap::new());
    // Map from button HWND -> widget handle (for WM_DRAWITEM lookup)
    #[cfg(target_os = "windows")]
    static BTN_HWND_TO_HANDLE: RefCell<HashMap<isize, i64>> = RefCell::new(HashMap::new());
}

/// Create a Button. Returns widget handle.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);
    let callback_ptr = unsafe { js_nanbox_get_pointer(on_press) } as *const u8;
    let control_id = alloc_control_id();

    #[cfg(target_os = "windows")]
    {
        let wide = to_wide(label);
        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(to_wide("BUTTON").as_ptr()),
                windows::core::PCWSTR(wide.as_ptr()),
                WINDOW_STYLE(BS_PUSHBUTTON as u32 | WS_CHILD.0 | WS_VISIBLE.0 | WS_TABSTOP.0),
                0, 0, 80, 30,
                super::get_parking_hwnd(),
                HMENU(control_id as *mut _),
                HINSTANCE::from(hinstance),
                None,
            ).unwrap();

            let handle = register_widget(hwnd, WidgetKind::Button, control_id);
            BUTTON_CALLBACKS.with(|cb| {
                cb.borrow_mut().insert(handle, callback_ptr);
            });
            handle
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = label;
        let handle = register_widget(0, WidgetKind::Button, control_id);
        BUTTON_CALLBACKS.with(|cb| {
            cb.borrow_mut().insert(handle, callback_ptr);
        });
        handle
    }
}

/// Handle button click (BN_CLICKED).
pub fn handle_click(handle: i64) {
    // Extract the callback pointer first, then drop the borrow before calling it.
    // The closure may create new buttons (borrowing BUTTON_CALLBACKS mutably).
    let ptr = BUTTON_CALLBACKS.with(|cb| {
        let callbacks = cb.borrow();
        callbacks.get(&handle).copied()
    });
    if let Some(ptr) = ptr {
        unsafe { js_closure_call0(ptr) };
    }
}

/// Set whether a Button has a visible border.
pub fn set_bordered(handle: i64, bordered: bool) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            unsafe {
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let new_style = if bordered {
                    style | BS_PUSHBUTTON as u32
                } else {
                    // Use BS_FLAT for a borderless look
                    (style & !(BS_PUSHBUTTON as u32)) | BS_FLAT as u32
                };
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, bordered);
    }
}

/// Set the title text of a Button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            let wide = to_wide(title);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, title);
    }
}

/// Set button image by SF Symbol name. On Windows, maps known names to Unicode/text fallbacks.
pub fn set_image(handle: i64, name_ptr: *const u8) {
    let name = str_from_header(name_ptr);
    let fallback = match name {
        "doc.on.doc" | "doc.on.doc.fill" => "\u{1F4C4}",
        "magnifyingglass" => "\u{1F50D}",
        "arrow.triangle.branch" => "\u{2442}",
        "ladybug" | "ladybug.fill" => "\u{1F41B}",
        "puzzlepiece.extension" | "puzzlepiece.extension.fill" => "\u{1F9E9}",
        "gearshape" | "gearshape.fill" | "gear" => "\u{2699}",
        "folder" | "folder.fill" => "\u{1F4C1}",
        "doc.text" | "doc.text.fill" => "\u{1F4C4}",
        "xmark" => "\u{2715}",
        "chevron.right" => "\u{203A}",
        "chevron.down" => "\u{2304}",
        "sidebar.left" | "sidebar.leading" => "\u{2261}",
        "plus" => "+",
        "ellipsis" => "\u{22EF}",
        _ => name,
    };

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            // Read existing title and prepend the icon
            let mut buf = [0u16; 512];
            let len = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
            let existing = if len > 0 {
                String::from_utf16_lossy(&buf[..len])
            } else {
                String::new()
            };
            let combined = if existing.is_empty() {
                fallback.to_string()
            } else {
                format!("{} {}", fallback, existing)
            };
            let wide = to_wide(&combined);
            unsafe {
                let _ = SetWindowTextW(hwnd, windows::core::PCWSTR(wide.as_ptr()));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, fallback);
    }
}

/// Set the text color of a button. Switches to owner-draw mode.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    let r_byte = (r * 255.0).round().min(255.0).max(0.0) as u32;
    let g_byte = (g * 255.0).round().min(255.0).max(0.0) as u32;
    let b_byte = (b * 255.0).round().min(255.0).max(0.0) as u32;
    let color = r_byte | (g_byte << 8) | (b_byte << 16);

    BUTTON_TEXT_COLORS.with(|c| c.borrow_mut().insert(handle, color));

    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = super::get_hwnd(handle) {
            BTN_HWND_TO_HANDLE.with(|m| m.borrow_mut().insert(hwnd.0 as isize, handle));
            unsafe {
                // Switch to owner-draw so we control text rendering
                let style = GetWindowLongW(hwnd, GWL_STYLE) as u32;
                let new_style = (style & !0x0F) | BS_OWNERDRAW as u32;
                SetWindowLongW(hwnd, GWL_STYLE, new_style as i32);
                let _ = InvalidateRect(hwnd, None, true);
            }
        }
    }
}

/// Handle WM_DRAWITEM for owner-draw buttons. Returns true if handled.
#[cfg(target_os = "windows")]
pub fn handle_draw_item(lparam: LPARAM) -> bool {
    let dis = unsafe { &*(lparam.0 as *const windows::Win32::UI::Controls::DRAWITEMSTRUCT) };
    let btn_hwnd_val = dis.hwndItem.0 as isize;

    let handle = BTN_HWND_TO_HANDLE.with(|m| m.borrow().get(&btn_hwnd_val).copied());
    let handle = match handle {
        Some(h) => h,
        None => return false,
    };

    let text_color = BUTTON_TEXT_COLORS.with(|c| c.borrow().get(&handle).copied());
    let text_color = match text_color {
        Some(c) => c,
        None => return false,
    };

    unsafe {
        let hdc = dis.hDC;
        let mut rect = dis.rcItem;

        // Fill background with parent container's bg color (or default)
        let parent_hwnd = GetParent(dis.hwndItem);
        let mut bg_filled = false;
        if let Ok(parent_hwnd) = parent_hwnd {
            let parent_handle = super::find_handle_by_hwnd(parent_hwnd);
            if parent_handle > 0 {
                if let Some(brush) = super::get_bg_brush(parent_handle) {
                    FillRect(hdc, &rect, brush);
                    bg_filled = true;
                }
            }
        }
        if !bg_filled {
            let bg_brush = windows::Win32::Graphics::Gdi::GetSysColorBrush(windows::Win32::Graphics::Gdi::COLOR_BTNFACE);
            FillRect(hdc, &rect, bg_brush);
        }

        // Draw text
        SetTextColor(hdc, COLORREF(text_color));
        SetBkMode(hdc, TRANSPARENT);

        // Use the button's font
        let hfont = windows::Win32::Graphics::Gdi::HFONT(
            SendMessageW(dis.hwndItem, WM_GETFONT, WPARAM(0), LPARAM(0)).0 as *mut _
        );
        let old_font = if !hfont.is_invalid() {
            SelectObject(hdc, hfont)
        } else {
            HGDIOBJ::default()
        };

        let text_len = GetWindowTextLengthW(dis.hwndItem);
        if text_len > 0 {
            let mut buf = vec![0u16; (text_len + 1) as usize];
            GetWindowTextW(dis.hwndItem, &mut buf);
            DrawTextW(hdc, &mut buf[..text_len as usize], &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }

        if !old_font.is_invalid() {
            SelectObject(hdc, old_font);
        }
    }
    true
}
