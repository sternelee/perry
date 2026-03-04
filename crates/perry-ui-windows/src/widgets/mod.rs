//! Widget registry — Vec<WidgetEntry> with 1-based handles.
//! Each widget has an HWND (on Windows), a kind, children list, and layout info.

pub mod text;
pub mod button;
pub mod vstack;
pub mod hstack;
pub mod spacer;
pub mod divider;
pub mod textfield;
pub mod toggle;
pub mod slider;
pub mod scrollview;
pub mod securefield;
pub mod progressview;
pub mod form;
pub mod zstack;
pub mod picker;
pub mod canvas;
pub mod navstack;
pub mod lazyvstack;
pub mod image;

use std::cell::RefCell;
use std::collections::HashMap;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::*;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::*;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Gdi::{CreateFontW, CreateRoundRectRgn, SetWindowRgn, InvalidateRect, HBRUSH, CreateSolidBrush, FillRect};
#[cfg(target_os = "windows")]
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;

#[derive(Clone, Debug, PartialEq)]
pub enum WidgetKind {
    Text,
    Button,
    VStack,
    HStack,
    Spacer,
    Divider,
    TextField,
    Toggle,
    Slider,
    ScrollView,
    SecureField,
    ProgressView,
    Form,
    Section,
    ZStack,
    Picker,
    Canvas,
    NavStack,
    LazyVStack,
    Image,
}

pub struct WidgetEntry {
    #[cfg(target_os = "windows")]
    pub hwnd: HWND,
    #[cfg(not(target_os = "windows"))]
    pub hwnd: isize,
    pub kind: WidgetKind,
    pub children: Vec<i64>,
    pub spacing: f64,
    pub insets: (f64, f64, f64, f64), // top, left, bottom, right
    pub hidden: bool,
    /// Win32 control ID (for WM_COMMAND routing)
    pub control_id: u16,
    /// When true, this widget absorbs remaining space in a VStack/HStack (like a Spacer).
    pub fills_remaining: bool,
    /// Fixed width in pixels (set by widgetSetWidth)
    pub fixed_width: Option<i32>,
}

/// Info returned by get_widget_info (clone-safe subset)
pub struct WidgetInfo {
    pub kind: WidgetKind,
    pub children: Vec<i64>,
    pub spacing: f64,
    pub insets: (f64, f64, f64, f64),
    pub hidden: bool,
    pub fills_remaining: bool,
    pub fixed_width: Option<i32>,
}

thread_local! {
    static WIDGETS: RefCell<Vec<WidgetEntry>> = RefCell::new(Vec::new());
    static NEXT_CONTROL_ID: RefCell<u16> = RefCell::new(1000);
    /// Hidden parking window used as a temporary parent for WS_CHILD widgets
    /// before they are reparented into the real window hierarchy.
    #[cfg(target_os = "windows")]
    static PARKING_HWND: RefCell<Option<HWND>> = RefCell::new(None);
    /// Background color brushes keyed by widget handle
    #[cfg(target_os = "windows")]
    static BG_BRUSHES: RefCell<HashMap<i64, HBRUSH>> = RefCell::new(HashMap::new());
    /// Background COLORREF values keyed by widget handle
    static BG_COLORS: RefCell<HashMap<i64, u32>> = RefCell::new(HashMap::new());
}

/// Convert RGB floats (0.0-1.0) to Win32 COLORREF (0x00BBGGRR)
#[cfg(target_os = "windows")]
fn rgb_to_colorref(r: f64, g: f64, b: f64) -> u32 {
    let r = (r * 255.0).round().min(255.0).max(0.0) as u32;
    let g = (g * 255.0).round().min(255.0).max(0.0) as u32;
    let b = (b * 255.0).round().min(255.0).max(0.0) as u32;
    r | (g << 8) | (b << 16)
}

/// Get the background brush for a widget (if set).
#[cfg(target_os = "windows")]
pub fn get_bg_brush(handle: i64) -> Option<HBRUSH> {
    BG_BRUSHES.with(|b| b.borrow().get(&handle).copied())
}

/// Get the background COLORREF for a widget (if set).
pub fn get_bg_color(handle: i64) -> Option<u32> {
    BG_COLORS.with(|c| c.borrow().get(&handle).copied())
}

/// Get (or lazily create) the hidden parking window for orphan child widgets.
#[cfg(target_os = "windows")]
pub fn get_parking_hwnd() -> HWND {
    fn to_wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    PARKING_HWND.with(|cell| {
        let mut opt = cell.borrow_mut();
        if let Some(hwnd) = *opt {
            return hwnd;
        }
        unsafe {
            let hinstance = windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap();
            let hinstance_h = HINSTANCE(hinstance.0 as _);
            // HWND_MESSAGE creates a message-only window (invisible, no UI)
            let class = to_wide("STATIC");
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                windows::core::PCWSTR(class.as_ptr()),
                windows::core::PCWSTR(std::ptr::null()),
                WINDOW_STYLE::default(),
                0, 0, 0, 0,
                HWND_MESSAGE,
                HMENU::default(),
                hinstance_h,
                None,
            ).unwrap();
            *opt = Some(hwnd);
            hwnd
        }
    })
}

/// Allocate a new control ID.
pub fn alloc_control_id() -> u16 {
    NEXT_CONTROL_ID.with(|id| {
        let mut id = id.borrow_mut();
        let current = *id;
        *id += 1;
        current
    })
}

/// Register a widget entry and return its 1-based handle.
#[cfg(target_os = "windows")]
pub fn register_widget(hwnd: HWND, kind: WidgetKind, control_id: u16) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing: 0.0,
            insets: (0.0, 0.0, 0.0, 0.0),
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
        });
        widgets.len() as i64
    })
}

#[cfg(not(target_os = "windows"))]
pub fn register_widget(hwnd: isize, kind: WidgetKind, control_id: u16) -> i64 {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing: 0.0,
            insets: (0.0, 0.0, 0.0, 0.0),
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
        });
        widgets.len() as i64
    })
}

/// Register a widget with spacing and insets (for stacks).
#[cfg(target_os = "windows")]
pub fn register_widget_with_layout(hwnd: HWND, kind: WidgetKind, spacing: f64, insets: (f64, f64, f64, f64)) -> i64 {
    let control_id = alloc_control_id();
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing,
            insets,
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
        });
        widgets.len() as i64
    })
}

#[cfg(not(target_os = "windows"))]
pub fn register_widget_with_layout(hwnd: isize, kind: WidgetKind, spacing: f64, insets: (f64, f64, f64, f64)) -> i64 {
    let control_id = alloc_control_id();
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        widgets.push(WidgetEntry {
            hwnd,
            kind,
            children: Vec::new(),
            spacing,
            insets,
            hidden: false,
            control_id,
            fills_remaining: false,
            fixed_width: None,
        });
        widgets.len() as i64
    })
}

/// Get the HWND for a widget handle.
#[cfg(target_os = "windows")]
pub fn get_hwnd(handle: i64) -> Option<HWND> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(widgets[idx].hwnd)
        } else {
            None
        }
    })
}

#[cfg(not(target_os = "windows"))]
pub fn get_hwnd(handle: i64) -> Option<isize> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(widgets[idx].hwnd)
        } else {
            None
        }
    })
}

/// Get widget info (clone-safe subset).
pub fn get_widget_info(handle: i64) -> Option<WidgetInfo> {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            Some(WidgetInfo {
                kind: widgets[idx].kind.clone(),
                children: widgets[idx].children.clone(),
                spacing: widgets[idx].spacing,
                insets: widgets[idx].insets,
                hidden: widgets[idx].hidden,
                fills_remaining: widgets[idx].fills_remaining,
                fixed_width: widgets[idx].fixed_width,
            })
        } else {
            None
        }
    })
}

/// Find the widget handle that owns a given HWND.
#[cfg(target_os = "windows")]
pub fn find_handle_by_hwnd(hwnd: HWND) -> i64 {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        for (i, widget) in widgets.iter().enumerate() {
            if widget.hwnd == hwnd {
                return (i + 1) as i64;
            }
        }
        0
    })
}

#[cfg(not(target_os = "windows"))]
pub fn find_handle_by_hwnd(_hwnd: isize) -> i64 { 0 }

/// Find widget handle by control ID.
pub fn find_handle_by_control_id(id: u16) -> i64 {
    WIDGETS.with(|w| {
        let widgets = w.borrow();
        for (i, widget) in widgets.iter().enumerate() {
            if widget.control_id == id {
                return (i + 1) as i64;
            }
        }
        0
    })
}

/// Add a child widget to a parent container.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    #[cfg(target_os = "windows")]
    {
        // Re-parent the child HWND
        if let (Some(parent_hwnd), Some(child_hwnd)) = (get_hwnd(parent_handle), get_hwnd(child_handle)) {
            unsafe {
                let _ = SetParent(child_hwnd, parent_hwnd);
                let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0 | WS_VISIBLE.0) as i32);
            }
        }
    }

    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].children.push(child_handle);
        }
    });
}

/// Add a child widget at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    #[cfg(target_os = "windows")]
    {
        if let (Some(parent_hwnd), Some(child_hwnd)) = (get_hwnd(parent_handle), get_hwnd(child_handle)) {
            unsafe {
                let _ = SetParent(child_hwnd, parent_hwnd);
                let style = GetWindowLongW(child_hwnd, GWL_STYLE) as u32;
                SetWindowLongW(child_hwnd, GWL_STYLE, (style | WS_CHILD.0 | WS_VISIBLE.0) as i32);
            }
        }
    }

    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            let insert_at = (index as usize).min(widgets[idx].children.len());
            widgets[idx].children.insert(insert_at, child_handle);
        }
    });
}

/// Remove a specific child from a parent container.
pub fn remove_child(parent_handle: i64, child_handle: i64) {
    // Remove from children list
    let removed = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (parent_handle - 1) as usize;
        if idx < widgets.len() {
            if let Some(pos) = widgets[idx].children.iter().position(|&c| c == child_handle) {
                widgets[idx].children.remove(pos);
                true
            } else {
                false
            }
        } else {
            false
        }
    });

    #[cfg(target_os = "windows")]
    {
        if removed {
            let parking = get_parking_hwnd();
            if let Some(child_hwnd) = get_hwnd(child_handle) {
                unsafe {
                    let _ = ShowWindow(child_hwnd, SW_HIDE);
                    let _ = SetParent(child_hwnd, parking);
                }
            }
        }
    }

    let _ = removed;
}

/// Remove all children from a container widget.
pub fn clear_children(handle: i64) {
    let children: Vec<i64> = WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].children.drain(..).collect()
        } else {
            Vec::new()
        }
    });

    #[cfg(target_os = "windows")]
    {
        let parking = get_parking_hwnd();
        for child in &children {
            if let Some(child_hwnd) = get_hwnd(*child) {
                unsafe {
                    let _ = ShowWindow(child_hwnd, SW_HIDE);
                    let _ = SetParent(child_hwnd, parking);
                }
            }
        }
    }

    let _ = children;
}

/// Mark a widget as filling remaining space in its parent VStack/HStack.
pub fn set_fills_remaining(handle: i64, fills: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].fills_remaining = fills;
        }
    });
}

/// Set the hidden state of a widget.
pub fn set_hidden(handle: i64, hidden: bool) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].hidden = hidden;

            #[cfg(target_os = "windows")]
            {
                let hwnd = widgets[idx].hwnd;
                unsafe {
                    let _ = ShowWindow(hwnd, if hidden { SW_HIDE } else { SW_SHOW });
                }
            }
        }
    });
}

/// Handle WM_COMMAND from WndProc — dispatch to button/textfield/toggle/picker/securefield callbacks.
#[cfg(target_os = "windows")]
pub fn handle_command(control_id: u16, notify_code: u16, _lparam: LPARAM) {
    // BN_CLICKED = 0
    if notify_code == 0 {
        // Could be a button click or toggle click
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = w.borrow();
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            match kind {
                Some(WidgetKind::Button) => button::handle_click(handle),
                Some(WidgetKind::Toggle) => toggle::handle_click(handle),
                _ => {}
            }
        }
    }
    // CBN_SELCHANGE = 1
    if notify_code == 1 {
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = w.borrow();
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            if matches!(kind, Some(WidgetKind::Picker)) {
                picker::handle_selchange(handle);
            }
        }
    }
    // EN_CHANGE = 0x0300
    if notify_code == 0x0300 {
        let handle = find_handle_by_control_id(control_id);
        if handle > 0 {
            let kind = WIDGETS.with(|w| {
                let widgets = w.borrow();
                let idx = (handle - 1) as usize;
                if idx < widgets.len() {
                    Some(widgets[idx].kind.clone())
                } else {
                    None
                }
            });
            match kind {
                Some(WidgetKind::SecureField) => securefield::handle_change(handle),
                _ => textfield::handle_change(handle),
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_command(_control_id: u16, _notify_code: u16, _lparam: isize) {}

/// Handle WM_HSCROLL/WM_VSCROLL — dispatch to slider or scrollview.
#[cfg(target_os = "windows")]
pub fn handle_scroll(wparam: WPARAM, lparam: LPARAM) {
    let child_hwnd = HWND(lparam.0 as *mut _);
    let handle = find_handle_by_hwnd(child_hwnd);
    if handle > 0 {
        let kind = WIDGETS.with(|w| {
            let widgets = w.borrow();
            let idx = (handle - 1) as usize;
            if idx < widgets.len() {
                Some(widgets[idx].kind.clone())
            } else {
                None
            }
        });
        match kind {
            Some(WidgetKind::Slider) => slider::handle_scroll(handle),
            _ => {}
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn handle_scroll(_wparam: usize, _lparam: isize) {}

// =============================================================================
// Property setters (new in parity update)
// =============================================================================

extern "C" {
    fn js_closure_call1(closure: *const u8, arg: f64) -> f64;
    fn js_nanbox_get_pointer(value: f64) -> i64;
}

/// Set the enabled/disabled state of a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe {
                let _ = EnableWindow(hwnd, enabled);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, enabled);
    }
}

/// Set the tooltip of a widget.
pub fn set_tooltip(handle: i64, _text_ptr: *const u8) {
    // Win32 tooltips require a shared TOOLTIPS_CLASS control with TTM_ADDTOOL.
    // For now, this is a best-effort no-op — full tooltip support would require
    // creating a shared tooltip window and managing per-widget TOOLINFO structs.
    let _ = handle;
}

/// Set the control size of a widget (maps to font size).
pub fn set_control_size(handle: i64, size: i64) {
    #[cfg(target_os = "windows")]
    {
        let font_height = match size {
            0 => 10, // mini
            1 => 12, // small
            2 => 14, // regular
            3 => 18, // large
            _ => 14,
        };
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe {
                let font = CreateFontW(
                    -font_height, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0,
                    windows::core::PCWSTR(
                        "Segoe UI\0".encode_utf16().collect::<Vec<u16>>().as_ptr(),
                    ),
                );
                SendMessageW(hwnd, WM_SETFONT, WPARAM(font.0 as usize), LPARAM(1));
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, size);
    }
}

/// Set the corner radius of a widget.
pub fn set_corner_radius(handle: i64, radius: f64) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe {
                let mut rect = RECT::default();
                let _ = GetClientRect(hwnd, &mut rect);
                let rgn = CreateRoundRectRgn(
                    0, 0,
                    rect.right + 1, rect.bottom + 1,
                    radius as i32, radius as i32,
                );
                SetWindowRgn(hwnd, rgn, true);
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, radius);
    }
}

/// Set the fixed width of a widget (in pixels).
pub fn set_fixed_width(handle: i64, width: i32) {
    WIDGETS.with(|w| {
        let mut widgets = w.borrow_mut();
        let idx = (handle - 1) as usize;
        if idx < widgets.len() {
            widgets[idx].fixed_width = Some(width);
        }
    });
}

/// Set hugging priority. Low priority (e.g. 1) means the widget should expand to fill space.
pub fn set_hugging_priority(handle: i64, priority: f64) {
    if priority <= 250.0 {
        set_fills_remaining(handle, true);
    }
}

/// Set the background color of a widget.
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, _a: f64) {
    #[cfg(target_os = "windows")]
    {
        let color = rgb_to_colorref(r, g, b);
        let brush = unsafe { CreateSolidBrush(COLORREF(color)) };
        BG_COLORS.with(|c| c.borrow_mut().insert(handle, color));
        BG_BRUSHES.with(|b| b.borrow_mut().insert(handle, brush));
        if let Some(hwnd) = get_hwnd(handle) {
            unsafe { let _ = InvalidateRect(hwnd, None, true); }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (handle, r, g, b, _a);
    }
}

/// Set the background gradient of a widget.
pub fn set_background_gradient(handle: i64, _r1: f64, _g1: f64, _b1: f64, _a1: f64, _r2: f64, _g2: f64, _b2: f64, _a2: f64, _direction: f64) {
    // Win32 gradient would require GradientFill() in WM_ERASEBKGND.
    // Best-effort no-op.
    let _ = handle;
}

/// Set an on-hover callback for a widget.
pub fn set_on_hover(handle: i64, _callback: f64) {
    // Win32 hover requires SetWindowSubclass + TrackMouseEvent + WM_MOUSEHOVER/LEAVE.
    // Best-effort no-op.
    let _ = handle;
}

/// Set a double-click callback for a widget.
pub fn set_on_double_click(handle: i64, _callback: f64) {
    // Win32 double-click requires CS_DBLCLKS style + WM_LBUTTONDBLCLK handling.
    // Best-effort no-op.
    let _ = handle;
}

/// Animate the opacity of a widget.
pub fn animate_opacity(handle: i64, _target: f64, _duration_ms: f64) {
    // Win32 opacity animation requires WS_EX_LAYERED + SetLayeredWindowAttributes + SetTimer.
    // Best-effort no-op.
    let _ = handle;
}

/// Animate the position of a widget.
pub fn animate_position(handle: i64, _dx: f64, _dy: f64, _duration_ms: f64) {
    // Win32 position animation requires SetTimer + incremental SetWindowPos.
    // Best-effort no-op.
    let _ = handle;
}
