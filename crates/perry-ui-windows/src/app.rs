//! App lifecycle — window creation, message pump, keyboard shortcuts (Win32)

use std::cell::RefCell;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::InitCommonControlsEx;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::INITCOMMONCONTROLSEX;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::ICC_STANDARD_CLASSES;
#[cfg(target_os = "windows")]
use windows::Win32::UI::Controls::ICC_BAR_CLASSES;
#[cfg(target_os = "windows")]
use windows::Win32::UI::HiDpi::SetProcessDpiAwarenessContext;
#[cfg(target_os = "windows")]
use windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2;

extern "C" {
    fn js_closure_call0(closure: *const u8) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
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

pub(crate) struct AppEntry {
    #[cfg(target_os = "windows")]
    pub(crate) hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    pub(crate) hwnd: isize,
    root_widget: Option<i64>,
    min_size: Option<(f64, f64)>,
    max_size: Option<(f64, f64)>,
}

struct PendingShortcut {
    key: String,
    modifiers: f64,
    callback: f64,
}

struct ShortcutEntry {
    vk: u16,
    ctrl: bool,
    shift: bool,
    alt: bool,
    callback_ptr: *const u8,
}

struct TimerEntry {
    interval_ms: u32,
    callback_ptr: *const u8,
}

thread_local! {
    pub(crate) static APPS: RefCell<Vec<AppEntry>> = RefCell::new(Vec::new());
    static PENDING_SHORTCUTS: RefCell<Vec<PendingShortcut>> = RefCell::new(Vec::new());
    static SHORTCUTS: RefCell<Vec<ShortcutEntry>> = RefCell::new(Vec::new());
    static TIMERS: RefCell<Vec<TimerEntry>> = RefCell::new(Vec::new());
    static ON_ACTIVATE_CALLBACK: RefCell<Option<*const u8>> = RefCell::new(None);
    static ON_TERMINATE_CALLBACK: RefCell<Option<*const u8>> = RefCell::new(None);
}

/// Get the HWND of the first (main) app window.
#[cfg(target_os = "windows")]
pub fn get_main_hwnd() -> Option<HWND> {
    APPS.with(|apps| {
        let apps = apps.borrow();
        apps.first().map(|a| a.hwnd)
    })
}

#[cfg(not(target_os = "windows"))]
pub fn get_main_hwnd() -> Option<isize> {
    None
}

#[cfg(target_os = "windows")]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Create an app window. Returns app handle (1-based).
pub fn app_create(title_ptr: *const u8, width: f64, height: f64) -> i64 {
    let title = str_from_header(title_ptr);
    let w = if width > 0.0 { width as i32 } else { 800 };
    let h = if height > 0.0 { height as i32 } else { 600 };

    #[cfg(target_os = "windows")]
    {
        unsafe {
            // Enable DPI awareness
            let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

            // Initialize common controls
            let icc = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_STANDARD_CLASSES | ICC_BAR_CLASSES,
            };
            let _ = InitCommonControlsEx(&icc);

            let hinstance = GetModuleHandleW(None).unwrap();

            // Register window class
            let class_name = to_wide("PerryMainWindow");
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wnd_proc),
                hInstance: HINSTANCE::from(hinstance),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut _),
                lpszClassName: windows::core::PCWSTR(class_name.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);

            let title_wide = to_wide(title);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class_name.as_ptr()),
                windows::core::PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT, CW_USEDEFAULT,
                w, h,
                None,
                None,
                HINSTANCE::from(hinstance),
                None,
            ).unwrap();

            // Attach any pending menu bar now that the window exists
            crate::menu::attach_pending_menubar(hwnd);

            APPS.with(|apps| {
                let mut apps = apps.borrow_mut();
                apps.push(AppEntry {
                    hwnd,
                    root_widget: None,
                    min_size: None,
                    max_size: None,
                });
                apps.len() as i64
            })
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (title, w, h);
        APPS.with(|apps| {
            let mut apps = apps.borrow_mut();
            apps.push(AppEntry {
                hwnd: 0,
                root_widget: None,
                min_size: None,
                max_size: None,
            });
            apps.len() as i64
        })
    }
}

/// Set the root widget of an app.
pub fn app_set_body(app_handle: i64, root_handle: i64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_widget = Some(root_handle);

            #[cfg(target_os = "windows")]
            {
                let hwnd = apps[idx].hwnd;
                // Set the root widget's HWND as a child of the main window
                if let Some(child_hwnd) = crate::widgets::get_hwnd(root_handle) {
                    unsafe {
                        let _ = SetParent(child_hwnd, hwnd);
                        let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                        SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0) as i32);
                        // Trigger initial layout
                        let mut rect = RECT::default();
                        let _ = GetClientRect(hwnd, &mut rect);
                        let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                        crate::layout::layout_widget(root_handle, rect.right, rect.bottom);
                    }
                }
            }
        }
    });
}

/// Run the app event loop (blocks until window closes).
pub fn app_run(app_handle: i64) {
    // Install pending keyboard shortcuts
    PENDING_SHORTCUTS.with(|pending| {
        let shortcuts: Vec<PendingShortcut> = pending.borrow_mut().drain(..).collect();
        for s in shortcuts {
            install_shortcut(&s.key, s.modifiers, s.callback);
        }
    });

    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            let idx = (app_handle - 1) as usize;
            if idx < apps.len() {
                unsafe {
                    let _ = ShowWindow(apps[idx].hwnd, SW_SHOW);
                    let _ = UpdateWindow(apps[idx].hwnd);
                }
            }
        });

        // Message loop
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                // Check keyboard shortcuts
                if msg.message == WM_KEYDOWN || msg.message == WM_SYSKEYDOWN {
                    if try_handle_shortcut(msg.wParam.0 as u16) {
                        continue;
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = app_handle;
    }
}

fn install_shortcut(key: &str, modifiers: f64, callback: f64) {
    let mods = modifiers as u32;
    // Perry modifier flags: 1=Cmd(->Ctrl on Win), 2=Shift, 4=Option(->Alt), 8=Control(->Ctrl)
    let ctrl = (mods & 1) != 0 || (mods & 8) != 0;
    let shift = (mods & 2) != 0;
    let alt = (mods & 4) != 0;

    let vk = key_to_vk(key);
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;

    SHORTCUTS.with(|s| {
        s.borrow_mut().push(ShortcutEntry {
            vk,
            ctrl,
            shift,
            alt,
            callback_ptr,
        });
    });
}

fn key_to_vk(key: &str) -> u16 {
    match key.to_lowercase().as_str() {
        "a" => 0x41, "b" => 0x42, "c" => 0x43, "d" => 0x44, "e" => 0x45,
        "f" => 0x46, "g" => 0x47, "h" => 0x48, "i" => 0x49, "j" => 0x4A,
        "k" => 0x4B, "l" => 0x4C, "m" => 0x4D, "n" => 0x4E, "o" => 0x4F,
        "p" => 0x50, "q" => 0x51, "r" => 0x52, "s" => 0x53, "t" => 0x54,
        "u" => 0x55, "v" => 0x56, "w" => 0x57, "x" => 0x58, "y" => 0x59,
        "z" => 0x5A,
        "0" => 0x30, "1" => 0x31, "2" => 0x32, "3" => 0x33, "4" => 0x34,
        "5" => 0x35, "6" => 0x36, "7" => 0x37, "8" => 0x38, "9" => 0x39,
        "return" | "enter" => 0x0D,
        "escape" | "esc" => 0x1B,
        "tab" => 0x09,
        "space" => 0x20,
        "delete" | "backspace" => 0x08,
        "up" => 0x26, "down" => 0x28, "left" => 0x25, "right" => 0x27,
        "f1" => 0x70, "f2" => 0x71, "f3" => 0x72, "f4" => 0x73,
        "f5" => 0x74, "f6" => 0x75, "f7" => 0x76, "f8" => 0x77,
        "f9" => 0x78, "f10" => 0x79, "f11" => 0x7A, "f12" => 0x7B,
        "home" => 0x24, "end" => 0x23,
        "pageup" => 0x21, "pagedown" => 0x22,
        _ => 0,
    }
}

fn try_handle_shortcut(vk: u16) -> bool {
    #[cfg(target_os = "windows")]
    {
        let ctrl_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11 /* VK_CONTROL */) } < 0;
        let shift_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10 /* VK_SHIFT */) } < 0;
        let alt_down = unsafe {
            windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x12 /* VK_MENU */) } < 0;

        SHORTCUTS.with(|shortcuts| {
            let shortcuts = shortcuts.borrow();
            for s in shortcuts.iter() {
                if s.vk == vk && s.ctrl == ctrl_down && s.shift == shift_down && s.alt == alt_down {
                    unsafe { js_closure_call0(s.callback_ptr) };
                    return true;
                }
            }
            false
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = vk;
        false
    }
}

/// Add a keyboard shortcut (buffered until app_run).
pub fn add_keyboard_shortcut(key_ptr: *const u8, modifiers: f64, callback: f64) {
    let key = str_from_header(key_ptr).to_string();
    PENDING_SHORTCUTS.with(|pending| {
        pending.borrow_mut().push(PendingShortcut {
            key,
            modifiers,
            callback,
        });
    });
}

/// Set minimum window size.
pub fn set_min_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].min_size = Some((w, h));
        }
    });
}

/// Set maximum window size.
pub fn set_max_size(app_handle: i64, w: f64, h: f64) {
    APPS.with(|apps| {
        let mut apps = apps.borrow_mut();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].max_size = Some((w, h));
        }
    });
}

/// Set a repeating timer.
pub fn set_timer(interval_ms: f64, callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    let ms = interval_ms as u32;

    TIMERS.with(|t| {
        let mut timers = t.borrow_mut();
        let timer_id = timers.len() + 1;
        timers.push(TimerEntry {
            interval_ms: ms,
            callback_ptr,
        });

        #[cfg(target_os = "windows")]
        {
            APPS.with(|apps| {
                let apps = apps.borrow();
                if let Some(app) = apps.first() {
                    unsafe {
                        let _ = SetTimer(app.hwnd, timer_id, ms, None);
                    }
                }
            });
        }

        let _ = timer_id;
    });
}

/// Register callback for app activation (WM_ACTIVATEAPP).
pub fn on_activate(callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    ON_ACTIVATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback_ptr);
    });
}

/// Register callback for app termination (WM_CLOSE/WM_DESTROY).
pub fn on_terminate(callback: f64) {
    let callback_ptr = unsafe { js_nanbox_get_pointer(callback) } as *const u8;
    ON_TERMINATE_CALLBACK.with(|c| {
        *c.borrow_mut() = Some(callback_ptr);
    });
}

/// Handle WM_TIMER — dispatch to registered timer callbacks.
#[cfg(target_os = "windows")]
pub fn handle_timer(timer_id: usize) {
    TIMERS.with(|t| {
        let timers = t.borrow();
        let idx = timer_id - 1;
        if idx < timers.len() {
            unsafe { js_closure_call0(timers[idx].callback_ptr) };
        }
    });
}

/// Handle WM_ACTIVATEAPP — call on_activate callback.
#[cfg(target_os = "windows")]
pub fn handle_activate(activating: bool) {
    if activating {
        ON_ACTIVATE_CALLBACK.with(|c| {
            if let Some(ptr) = *c.borrow() {
                unsafe { js_closure_call0(ptr) };
            }
        });
    }
}

/// Handle WM_DESTROY — call on_terminate callback before quit.
#[cfg(target_os = "windows")]
pub fn handle_terminate() {
    ON_TERMINATE_CALLBACK.with(|c| {
        if let Some(ptr) = *c.borrow() {
            unsafe { js_closure_call0(ptr) };
        }
    });
}

/// Get app min/max sizes for WM_GETMINMAXINFO.
pub fn get_size_constraints(app_handle: i64) -> (Option<(f64, f64)>, Option<(f64, f64)>) {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            (apps[idx].min_size, apps[idx].max_size)
        } else {
            (None, None)
        }
    })
}

/// Get root widget handle for an app.
pub fn get_root_widget(app_handle: i64) -> Option<i64> {
    APPS.with(|apps| {
        let apps = apps.borrow();
        let idx = (app_handle - 1) as usize;
        if idx < apps.len() {
            apps[idx].root_widget
        } else {
            None
        }
    })
}

/// Custom message ID for deferred layout requests.
#[cfg(target_os = "windows")]
const WM_PERRY_LAYOUT: u32 = WM_USER + 100;

/// Request a deferred layout pass on the main window's root widget.
/// Uses PostMessage so multiple rapid calls (e.g., during tree rebuild)
/// are coalesced into a single layout pass in the message loop.
pub fn request_layout() {
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            if let Some(app) = apps.first() {
                if app.root_widget.is_some() {
                    unsafe {
                        let _ = PostMessageW(app.hwnd, WM_PERRY_LAYOUT, WPARAM(0), LPARAM(0));
                    }
                }
            }
        });
    }
}

/// Perform an immediate layout pass on the main window's root widget.
fn do_layout() {
    #[cfg(target_os = "windows")]
    {
        APPS.with(|apps| {
            let apps = apps.borrow();
            if let Some(app) = apps.first() {
                if let Some(root) = app.root_widget {
                    unsafe {
                        let mut rect = RECT::default();
                        let _ = GetClientRect(app.hwnd, &mut rect);
                        if rect.right > 0 && rect.bottom > 0 {
                            if let Some(child_hwnd) = crate::widgets::get_hwnd(root) {
                                let _ = MoveWindow(child_hwnd, 0, 0, rect.right, rect.bottom, true);
                                crate::layout::layout_widget(root, rect.right, rect.bottom);
                            }
                            let _ = InvalidateRect(app.hwnd, None, true);
                        }
                    }
                }
            }
        });
    }
}

// =============================================================================
// WndProc — Win32 message handler
// =============================================================================

#[cfg(target_os = "windows")]
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SIZE => {
            let width = (lparam.0 & 0xFFFF) as i32;
            let height = ((lparam.0 >> 16) & 0xFFFF) as i32;
            // Resize root widget to fill window
            if let Some(root) = get_root_widget(1) {
                if let Some(child_hwnd) = crate::widgets::get_hwnd(root) {
                    let _ = MoveWindow(child_hwnd, 0, 0, width, height, true);
                    crate::layout::layout_widget(root, width, height);
                }
            }
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let info = &mut *(lparam.0 as *mut MINMAXINFO);
            let (min, max) = get_size_constraints(1);
            if let Some((w, h)) = min {
                info.ptMinTrackSize = POINT { x: w as i32, y: h as i32 };
            }
            if let Some((w, h)) = max {
                info.ptMaxTrackSize = POINT { x: w as i32, y: h as i32 };
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let control_id = (wparam.0 & 0xFFFF) as u16;
            let notify_code = ((wparam.0 >> 16) & 0xFFFF) as u16;
            if notify_code == 0 && lparam.0 == 0 {
                // Menu bar command — lparam==0 distinguishes from control notifications
                crate::menu::dispatch_menu_item(control_id);
            } else {
                crate::widgets::handle_command(control_id, notify_code, lparam);
            }
            LRESULT(0)
        }
        x if x == WM_HSCROLL || x == WM_VSCROLL => {
            crate::widgets::handle_scroll(wparam, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut _);
            let child_hwnd = HWND(lparam.0 as *mut _);
            if let Some(result) = crate::widgets::text::handle_ctlcolor(hdc, child_hwnd) {
                return result;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CTLCOLORBTN => {
            // Make buttons use their parent container's background color
            let hdc = HDC(wparam.0 as *mut _);
            let btn_hwnd = HWND(lparam.0 as *mut _);
            if let Ok(parent_hwnd) = GetParent(btn_hwnd) {
                let parent_handle = crate::widgets::find_handle_by_hwnd(parent_hwnd);
                if parent_handle > 0 {
                    if let (Some(color), Some(brush)) = (
                        crate::widgets::get_bg_color(parent_handle),
                        crate::widgets::get_bg_brush(parent_handle),
                    ) {
                        SetBkColor(hdc, COLORREF(color));
                        SetBkMode(hdc, TRANSPARENT);
                        return LRESULT(brush.0 as isize);
                    }
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DRAWITEM => {
            if crate::widgets::button::handle_draw_item(lparam) {
                return LRESULT(1); // TRUE = handled
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_CONTEXTMENU => {
            let child_hwnd = HWND(wparam.0 as *mut _);
            let x = (lparam.0 & 0xFFFF) as i16 as i32;
            let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
            crate::menu::handle_context_menu(hwnd, child_hwnd, x, y);
            LRESULT(0)
        }
        WM_TIMER => {
            let timer_id = wparam.0;
            crate::app::handle_timer(timer_id);
            LRESULT(0)
        }
        WM_ACTIVATEAPP => {
            let activating = wparam.0 != 0;
            crate::app::handle_activate(activating);
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_DESTROY => {
            crate::app::handle_terminate();
            PostQuitMessage(0);
            LRESULT(0)
        }
        x if x == WM_PERRY_LAYOUT => {
            do_layout();
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
