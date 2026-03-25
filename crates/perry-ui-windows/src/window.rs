//! Multi-window support (Win32)

use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    #[cfg(target_os = "windows")]
    static WINDOWS: RefCell<HashMap<i64, windows::Win32::Foundation::HWND>> = RefCell::new(HashMap::new());
    #[cfg(not(target_os = "windows"))]
    static WINDOWS: RefCell<HashMap<i64, isize>> = RefCell::new(HashMap::new());
    static NEXT_WINDOW_ID: RefCell<i64> = RefCell::new(1);
}

fn str_from_header(ptr: *const u8) -> &'static str {
    if ptr.is_null() { return ""; }
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

#[cfg(target_os = "windows")]
unsafe extern "system" fn window_default_wnd_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
) -> windows::Win32::Foundation::LRESULT {
    windows::Win32::UI::WindowsAndMessaging::DefWindowProcW(hwnd, msg, wparam, lparam)
}

/// Create a new window.
pub fn create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = str_from_header(title_ptr);
    let id = NEXT_WINDOW_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    });

    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::*;
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::Graphics::Gdi::{HBRUSH, COLOR_WINDOW, UpdateWindow};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::core::PCWSTR;

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap();
            let class_name = to_wide("PerryWindow");

            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(window_default_wnd_proc),
                hInstance: hinstance.into(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let title_wide = to_wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT, CW_USEDEFAULT,
                width as i32, height as i32,
                None, None,
                HINSTANCE::from(hinstance),
                None,
            ).unwrap();

            WINDOWS.with(|w| w.borrow_mut().insert(id, hwnd));
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, width, height);
        WINDOWS.with(|w| w.borrow_mut().insert(id, 0));
    }

    id
}

/// Set the body (root widget) of a window.
pub fn set_body(window_handle: i64, widget_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(parent_hwnd) = w.borrow().get(&window_handle) {
                if let Some(child_hwnd) = crate::widgets::get_hwnd(widget_handle) {
                    unsafe {
                        let _ = SetParent(child_hwnd, *parent_hwnd);
                        let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                        SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0) as i32);
                    }
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = (window_handle, widget_handle); }
}

/// Show a window.
pub fn show(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::Graphics::Gdi::UpdateWindow;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = ShowWindow(*hwnd, SW_SHOW);
                    let _ = UpdateWindow(*hwnd);
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = window_handle; }
}

/// Close a window.
pub fn close(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe { let _ = DestroyWindow(*hwnd); }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = window_handle; }
}

thread_local! {
    pub(crate) static FOCUS_LOST_CALLBACKS: RefCell<HashMap<i64, f64>> = RefCell::new(HashMap::new());
}

/// Hide a window without destroying it.
pub fn hide(window_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe { let _ = ShowWindow(*hwnd, SW_HIDE); }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = window_handle; }
}

/// Set window size.
pub fn set_size(window_handle: i64, width: f64, height: f64) {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::UI::WindowsAndMessaging::*;
        WINDOWS.with(|w| {
            if let Some(hwnd) = w.borrow().get(&window_handle) {
                unsafe {
                    let _ = SetWindowPos(
                        *hwnd, None,
                        0, 0,
                        width as i32, height as i32,
                        SWP_NOMOVE | SWP_NOZORDER,
                    );
                }
            }
        });
    }
    #[cfg(not(target_os = "windows"))]
    { let _ = (window_handle, width, height); }
}

/// Register a callback for focus loss. Store it and handle in wndproc WM_ACTIVATE.
pub fn on_focus_lost(window_handle: i64, callback: f64) {
    FOCUS_LOST_CALLBACKS.with(|cbs| {
        cbs.borrow_mut().insert(window_handle, callback);
    });
}
