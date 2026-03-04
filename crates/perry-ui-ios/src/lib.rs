pub mod app;
pub mod clipboard;
pub mod file_dialog;
pub mod location;
pub mod menu;
pub mod state;
pub mod widgets;

// =============================================================================
// FFI exports — identical signatures to perry-ui-macos
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_create(title_ptr: i64, width: f64, height: f64) -> i64 {
    app::app_create(title_ptr as *const u8, width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_body(app_handle: i64, root_handle: i64) {
    app::app_set_body(app_handle, root_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_run(app_handle: i64) {
    app::app_run(app_handle);
}

/// Register an external UIView (from a native library) as a Perry widget.
/// Alias of perry_ui_embed_nsview so cross-platform Perry code works unchanged on iOS.
#[no_mangle]
pub extern "C" fn perry_ui_embed_nsview(uiview_ptr: i64) -> i64 {
    use objc2::rc::Retained;
    use objc2_ui_kit::UIView;
    if uiview_ptr == 0 {
        return 0;
    }
    match unsafe { Retained::retain(uiview_ptr as *mut UIView) } {
        Some(view) => widgets::register_widget(view),
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn perry_ui_text_create(text_ptr: i64) -> i64 {
    widgets::text::create(text_ptr as *const u8)
}

#[no_mangle]
pub extern "C" fn perry_ui_button_create(label_ptr: i64, on_press: f64) -> i64 {
    widgets::button::create(label_ptr as *const u8, on_press)
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create(spacing: f64) -> i64 {
    widgets::vstack::create(spacing)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create(spacing: f64) -> i64 {
    widgets::hstack::create(spacing)
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child(parent_handle: i64, child_handle: i64) {
    widgets::add_child(parent_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_create(initial: f64) -> i64 {
    state::state_create(initial)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_get(state_handle: i64) -> f64 {
    state::state_get(state_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_state_set(state_handle: i64, value: f64) {
    state::state_set(state_handle, value);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_numeric(state_handle: i64, text_handle: i64, prefix_ptr: i64, suffix_ptr: i64) {
    state::bind_text_numeric(state_handle, text_handle, prefix_ptr as *const u8, suffix_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_spacer_create() -> i64 {
    widgets::spacer::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_divider_create() -> i64 {
    widgets::divider::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::textfield::create(placeholder_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_toggle_create(label_ptr: i64, on_change: f64) -> i64 {
    widgets::toggle::create(label_ptr as *const u8, on_change)
}

#[no_mangle]
pub extern "C" fn perry_ui_slider_create(min: f64, max: f64, initial: f64, on_change: f64) -> i64 {
    widgets::slider::create(min, max, initial, on_change)
}

// =============================================================================
// Phase 4: Advanced Reactive UI
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_slider(state_handle: i64, slider_handle: i64) {
    state::bind_slider(state_handle, slider_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_toggle(state_handle: i64, toggle_handle: i64) {
    state::bind_toggle(state_handle, toggle_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_text_template(
    text_handle: i64,
    num_parts: i32,
    types_ptr: i64,
    values_ptr: i64,
) {
    state::bind_text_template(text_handle, num_parts, types_ptr as *const i32, values_ptr as *const i64);
}

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_visibility(state_handle: i64, show_handle: i64, hide_handle: i64) {
    state::bind_visibility(state_handle, show_handle, hide_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_set_widget_hidden(handle: i64, hidden: i64) {
    widgets::set_hidden(handle, hidden != 0);
}

#[no_mangle]
pub extern "C" fn perry_ui_for_each_init(container_handle: i64, state_handle: i64, render_closure: f64) {
    state::for_each_init(container_handle, state_handle, render_closure);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_clear_children(handle: i64) {
    widgets::clear_children(handle);
}

// =============================================================================
// Phase A.1: Text Mutation & Layout Control
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_string(handle: i64, text_ptr: i64) {
    widgets::text::set_string(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_vstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    widgets::vstack::create_with_insets(spacing, top, left, bottom, right)
}

#[no_mangle]
pub extern "C" fn perry_ui_hstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64 {
    widgets::hstack::create_with_insets(spacing, top, left, bottom, right)
}

// =============================================================================
// Phase A.2: ScrollView, Clipboard & Keyboard Shortcuts
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_create() -> i64 {
    widgets::scrollview::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_child(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::set_child(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_read() -> f64 {
    clipboard::read()
}

#[no_mangle]
pub extern "C" fn perry_ui_clipboard_write(text_ptr: i64) {
    clipboard::write(text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64) {
    app::add_keyboard_shortcut(key_ptr as *const u8, modifiers, callback);
}

// =============================================================================
// Phase A.3: Text Styling & Button Styling
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::text::set_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_size(handle: i64, size: f64) {
    widgets::text::set_font_size(handle, size);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_weight(handle: i64, size: f64, weight: f64) {
    widgets::text::set_font_weight(handle, size, weight);
}

#[no_mangle]
pub extern "C" fn perry_ui_text_set_selectable(handle: i64, selectable: f64) {
    widgets::text::set_selectable(handle, selectable != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_bordered(handle: i64, bordered: f64) {
    widgets::button::set_bordered(handle, bordered != 0.0);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_title(handle: i64, title_ptr: i64) {
    widgets::button::set_title(handle, title_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_button_set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::button::set_text_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_width(handle: i64, width: f64) {
    widgets::set_width(handle, width);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_hugging(handle: i64, priority: f64) {
    widgets::set_hugging_priority(handle, priority);
}

// =============================================================================
// Phase A.4: Focus & Scroll-To
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_textfield_focus(handle: i64) {
    widgets::textfield::focus(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_scroll_to(scroll_handle: i64, child_handle: i64) {
    widgets::scrollview::scroll_to(scroll_handle, child_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_get_offset(scroll_handle: i64) -> f64 {
    widgets::scrollview::get_offset(scroll_handle)
}

#[no_mangle]
pub extern "C" fn perry_ui_scrollview_set_offset(scroll_handle: i64, offset: f64) {
    widgets::scrollview::set_offset(scroll_handle, offset);
}

// =============================================================================
// Phase A.5: Context Menus, File Dialog & Window Sizing
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_menu_create() -> i64 {
    menu::create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item(menu_handle: i64, title_ptr: i64, callback: f64) {
    menu::add_item(menu_handle, title_ptr as *const u8, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_context_menu(widget_handle: i64, menu_handle: i64) {
    menu::set_context_menu(widget_handle, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_item_with_shortcut(menu_handle: i64, title_ptr: i64, callback: f64, shortcut_ptr: i64) {
    menu::add_item_with_shortcut(menu_handle, title_ptr as *const u8, callback, shortcut_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_separator(menu_handle: i64) {
    menu::add_separator(menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menu_add_submenu(menu_handle: i64, title_ptr: i64, submenu_handle: i64) {
    menu::add_submenu(menu_handle, title_ptr as *const u8, submenu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_create() -> i64 {
    menu::menubar_create()
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_add_menu(bar_handle: i64, title_ptr: i64, menu_handle: i64) {
    menu::menubar_add_menu(bar_handle, title_ptr as *const u8, menu_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_menubar_attach(bar_handle: i64) {
    menu::menubar_attach(bar_handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_open_file_dialog(callback: f64) {
    file_dialog::open_dialog(callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_min_size(app_handle: i64, w: f64, h: f64) {
    app::set_min_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_app_set_max_size(app_handle: i64, w: f64, h: f64) {
    app::set_max_size(app_handle, w, h);
}

#[no_mangle]
pub extern "C" fn perry_ui_textfield_set_string(handle: i64, text_ptr: i64) {
    widgets::textfield::set_string_value(handle, text_ptr as *const u8);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_add_child_at(parent_handle: i64, child_handle: i64, index: f64) {
    widgets::add_child_at(parent_handle, child_handle, index as i64);
}

// =============================================================================
// Timer, Background Styling & Canvas
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_set_timer(interval_ms: f64, callback: f64) {
    app::set_timer(interval_ms, callback);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_color(
    handle: i64, r: f64, g: f64, b: f64, a: f64,
) {
    widgets::set_background_color(handle, r, g, b, a);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_background_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    widgets::set_background_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

#[no_mangle]
pub extern "C" fn perry_ui_widget_set_corner_radius(handle: i64, radius: f64) {
    widgets::set_corner_radius(handle, radius);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_create(width: f64, height: f64) -> i64 {
    widgets::canvas::create(width, height)
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_clear(handle: i64) {
    widgets::canvas::clear(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_begin_path(handle: i64) {
    widgets::canvas::begin_path(handle);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_move_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::move_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_line_to(handle: i64, x: f64, y: f64) {
    widgets::canvas::line_to(handle, x, y);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_stroke(
    handle: i64, r: f64, g: f64, b: f64, a: f64, line_width: f64,
) {
    widgets::canvas::stroke(handle, r, g, b, a, line_width);
}

#[no_mangle]
pub extern "C" fn perry_ui_canvas_fill_gradient(
    handle: i64, r1: f64, g1: f64, b1: f64, a1: f64,
    r2: f64, g2: f64, b2: f64, a2: f64, direction: f64,
) {
    widgets::canvas::fill_gradient(handle, r1, g1, b1, a1, r2, g2, b2, a2, direction);
}

// =============================================================================
// New Widgets: SecureField, ProgressView, Image, Picker, Form, NavStack, ZStack
// =============================================================================

/// Create a SecureField (password input). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_securefield_create(placeholder_ptr: i64, on_change: f64) -> i64 {
    widgets::securefield::create(placeholder_ptr as *const u8, on_change)
}

/// Create an indeterminate ProgressView (spinner). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_progressview_create() -> i64 {
    widgets::progressview::create()
}

/// Set determinate progress value (0.0-1.0).
#[no_mangle]
pub extern "C" fn perry_ui_progressview_set_value(handle: i64, value: f64) {
    widgets::progressview::set_value(handle, value);
}

/// Create an Image from an SF Symbol name. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_symbol(name_ptr: i64) -> i64 {
    widgets::image::create_symbol(name_ptr as *const u8)
}

/// Create an Image from a file path. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_image_create_file(path_ptr: i64) -> i64 {
    widgets::image::create_file(path_ptr as *const u8)
}

/// Set the size of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_size(handle: i64, width: f64, height: f64) {
    widgets::image::set_size(handle, width, height);
}

/// Set the tint color of an Image widget.
#[no_mangle]
pub extern "C" fn perry_ui_image_set_tint(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    widgets::image::set_tint(handle, r, g, b, a);
}

/// Create a Picker (dropdown). style: 0=dropdown, 1=segmented. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_picker_create(label_ptr: i64, on_change: f64, style: i64) -> i64 {
    widgets::picker::create(label_ptr as *const u8, on_change, style)
}

/// Add an item to a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_add_item(handle: i64, title_ptr: i64) {
    widgets::picker::add_item(handle, title_ptr as *const u8);
}

/// Set the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_set_selected(handle: i64, index: i64) {
    widgets::picker::set_selected(handle, index);
}

/// Get the selected index of a Picker.
#[no_mangle]
pub extern "C" fn perry_ui_picker_get_selected(handle: i64) -> i64 {
    widgets::picker::get_selected(handle)
}

/// Create a TabBar. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_create(on_change: f64) -> i64 {
    widgets::tabbar::create(on_change)
}

/// Add a tab to a TabBar.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_add_tab(handle: i64, label_ptr: i64) {
    widgets::tabbar::add_tab(handle, label_ptr as *const u8);
}

/// Set the selected tab index.
#[no_mangle]
pub extern "C" fn perry_ui_tabbar_set_selected(handle: i64, index: i64) {
    widgets::tabbar::set_selected(handle, index);
}

/// Create a Form container. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_form_create() -> i64 {
    widgets::form::form_create()
}

/// Create a Section with title. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_section_create(title_ptr: i64) -> i64 {
    widgets::form::section_create(title_ptr as *const u8)
}

/// Create a NavigationStack. Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_create(title_ptr: i64, body_handle: i64) -> i64 {
    widgets::navstack::create(title_ptr as *const u8, body_handle)
}

/// Push a view onto the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_push(handle: i64, title_ptr: i64, body_handle: i64) {
    widgets::navstack::push(handle, title_ptr as *const u8, body_handle);
}

/// Pop the top view from the NavigationStack.
#[no_mangle]
pub extern "C" fn perry_ui_navstack_pop(handle: i64) {
    widgets::navstack::pop(handle);
}

/// Create a ZStack (overlay layout). Returns widget handle.
#[no_mangle]
pub extern "C" fn perry_ui_zstack_create() -> i64 {
    widgets::zstack::create()
}

// =============================================================================
// Cross-cutting: Enabled, Hover, DoubleClick, Animations, Tooltip, ControlSize
// =============================================================================

/// Set the enabled state of a widget. enabled: 0=disabled, 1=enabled.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_enabled(handle: i64, enabled: i64) {
    widgets::set_enabled(handle, enabled != 0);
}

/// Set a tooltip on a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_tooltip(handle: i64, text_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).length as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    widgets::set_tooltip(handle, str_from_header(text_ptr as *const u8));
}

/// Set the control size of a widget. 0=regular, 1=small, 2=mini, 3=large.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_control_size(handle: i64, size: i64) {
    widgets::set_control_size(handle, size);
}

/// Set an on-hover callback for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_hover(handle: i64, callback: f64) {
    widgets::set_on_hover(handle, callback);
}

/// Set a double-click/tap handler for a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_on_double_click(handle: i64, callback: f64) {
    widgets::set_on_double_click(handle, callback);
}

/// Animate the opacity of a widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_opacity(handle: i64, target: f64, duration_ms: f64) {
    widgets::animate_opacity(handle, target, duration_ms);
}

/// Animate the position of a widget by delta.
#[no_mangle]
pub extern "C" fn perry_ui_widget_animate_position(handle: i64, dx: f64, dy: f64, duration_ms: f64) {
    widgets::animate_position(handle, dx, dy, duration_ms);
}

/// Register an onChange callback for a state cell.
#[no_mangle]
pub extern "C" fn perry_ui_state_on_change(state_handle: i64, callback: f64) {
    state::state_on_change(state_handle, callback);
}

// =============================================================================
// System APIs (perry/system module)
// =============================================================================

/// Open a URL in the default browser/app.
#[no_mangle]
pub extern "C" fn perry_system_open_url(url_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).length as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let url_str = str_from_header(url_ptr as *const u8);
    unsafe {
        let ns_url_str = objc2_foundation::NSString::from_str(url_str);
        let url_cls = objc2::runtime::AnyClass::get(c"NSURL").unwrap();
        let url: *mut objc2::runtime::AnyObject = objc2::msg_send![url_cls, URLWithString: &*ns_url_str];
        if !url.is_null() {
            let app_cls = objc2::runtime::AnyClass::get(c"UIApplication").unwrap();
            let app: *mut objc2::runtime::AnyObject = objc2::msg_send![app_cls, sharedApplication];
            let _: () = objc2::msg_send![app, openURL: url];
        }
    }
}

/// Request one-shot location. Callback receives (lat, lon) or (NaN, NaN) on error.
#[no_mangle]
pub extern "C" fn perry_system_request_location(callback: f64) {
    location::request_location(callback);
}

/// Check if dark mode is active. Returns 1 if dark, 0 if light.
#[no_mangle]
pub extern "C" fn perry_system_is_dark_mode() -> i64 {
    unsafe {
        let tc_cls = objc2::runtime::AnyClass::get(c"UITraitCollection").unwrap();
        let tc: *mut objc2::runtime::AnyObject = objc2::msg_send![tc_cls, currentTraitCollection];
        if tc.is_null() { return 0; }
        let style: i64 = objc2::msg_send![tc, userInterfaceStyle];
        if style == 2 { 1 } else { 0 } // 2 = UIUserInterfaceStyleDark
    }
}

/// Set a preference value (UserDefaults).
#[no_mangle]
pub extern "C" fn perry_system_preferences_set(key_ptr: i64, value: f64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).length as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    extern "C" {
        fn js_nanbox_get_pointer(value: f64) -> i64;
    }
    let key = str_from_header(key_ptr as *const u8);
    let bits = value.to_bits();
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        if (bits >> 48) == 0x7FFF {
            let str_ptr = js_nanbox_get_pointer(value) as *const u8;
            let s = str_from_header(str_ptr);
            let ns_str = objc2_foundation::NSString::from_str(s);
            let _: () = objc2::msg_send![defaults, setObject: &*ns_str, forKey: &*ns_key];
        } else {
            let ns_num: objc2::rc::Retained<objc2::runtime::AnyObject> = objc2::msg_send![
                objc2::runtime::AnyClass::get(c"NSNumber").unwrap(), numberWithDouble: value
            ];
            let _: () = objc2::msg_send![defaults, setObject: &*ns_num, forKey: &*ns_key];
        }
    }
}

/// Get a preference value (UserDefaults). Returns NaN-boxed value or TAG_UNDEFINED.
#[no_mangle]
pub extern "C" fn perry_system_preferences_get(key_ptr: i64) -> f64 {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).length as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    extern "C" {
        fn js_string_from_bytes(ptr: *const u8, len: i64) -> *const u8;
        fn js_nanbox_string(ptr: i64) -> f64;
    }
    let key = str_from_header(key_ptr as *const u8);
    unsafe {
        let defaults_cls = objc2::runtime::AnyClass::get(c"NSUserDefaults").unwrap();
        let defaults: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults_cls, standardUserDefaults];
        let ns_key = objc2_foundation::NSString::from_str(key);
        let obj: *mut objc2::runtime::AnyObject = objc2::msg_send![defaults, objectForKey: &*ns_key];
        if obj.is_null() {
            return f64::from_bits(0x7FFC_0000_0000_0001);
        }
        if let Some(str_cls) = objc2::runtime::AnyClass::get(c"NSString") {
            let is_string: bool = objc2::msg_send![obj, isKindOfClass: str_cls];
            if is_string {
                let ns_str: &objc2_foundation::NSString = &*(obj as *const objc2_foundation::NSString);
                let rust_str = ns_str.to_string();
                let bytes = rust_str.as_bytes();
                let str_ptr = js_string_from_bytes(bytes.as_ptr(), bytes.len() as i64);
                return js_nanbox_string(str_ptr as i64);
            }
        }
        if let Some(num_cls) = objc2::runtime::AnyClass::get(c"NSNumber") {
            let is_number: bool = objc2::msg_send![obj, isKindOfClass: num_cls];
            if is_number {
                let val: f64 = objc2::msg_send![obj, doubleValue];
                return val;
            }
        }
        f64::from_bits(0x7FFC_0000_0000_0001)
    }
}

/// Set the font family on a Text widget.
#[no_mangle]
pub extern "C" fn perry_ui_text_set_font_family(handle: i64, family_ptr: i64) {
    fn str_from_header(ptr: *const u8) -> &'static str {
        if ptr.is_null() { return ""; }
        unsafe {
            let header = ptr as *const perry_runtime::string::StringHeader;
            let len = (*header).length as usize;
            let data = ptr.add(std::mem::size_of::<perry_runtime::string::StringHeader>());
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(data, len))
        }
    }
    let family = str_from_header(family_ptr as *const u8);
    if let Some(view) = widgets::get_widget(handle) {
        unsafe {
            let size: f64 = objc2::msg_send![&*view, font];
            let size = 13.0f64; // Default size for iOS
            let font: objc2::rc::Retained<objc2::runtime::AnyObject> = if family == "monospaced" || family == "monospace" {
                objc2::msg_send![
                    objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                    monospacedSystemFontOfSize: size,
                    weight: 0.0f64
                ]
            } else {
                let ns_name = objc2_foundation::NSString::from_str(family);
                objc2::msg_send![
                    objc2::runtime::AnyClass::get(c"UIFont").unwrap(),
                    fontWithName: &*ns_name,
                    size: size
                ]
            };
            let _: () = objc2::msg_send![&*view, setFont: &*font];
        }
    }
}

// =============================================================================
// Save File Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_save_file_dialog(_callback: f64, _default_name: i64, _allowed_types: i64) {
    // iOS: UIDocumentPickerViewController needed — stub for now
}

// =============================================================================
// State TextField Binding
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_state_bind_textfield(state_handle: i64, textfield_handle: i64) {
    state::bind_textfield(state_handle, textfield_handle);
}

// =============================================================================
// Alert Dialog
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_alert(_title: i64, _message: i64, _buttons: i64, _callback: f64) {
    // iOS: UIAlertController — stub for now
}

// =============================================================================
// Sheet
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_sheet_create(_width: f64, _height: f64, _title: i64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_present(_sheet: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_sheet_dismiss(_sheet: i64) {}

// =============================================================================
// App Lifecycle
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_app_on_terminate(_callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_app_on_activate(_callback: f64) {}

// =============================================================================
// Toolbar
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_create() -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_add_item(_toolbar: i64, _label: i64, _icon: i64, _callback: f64) {}

#[no_mangle]
pub extern "C" fn perry_ui_toolbar_attach(_toolbar: i64) {}

// =============================================================================
// Keychain
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_keychain_save(_key: i64, _value: i64) {}

#[no_mangle]
pub extern "C" fn perry_system_keychain_get(_key: i64) -> f64 {
    f64::from_bits(0x7FFC_0000_0000_0001) // TAG_UNDEFINED
}

#[no_mangle]
pub extern "C" fn perry_system_keychain_delete(_key: i64) {}

// =============================================================================
// Notifications
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_system_notification_send(_title: i64, _body: i64) {}

// =============================================================================
// Multi-Window
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_window_create(_title: i64, _width: f64, _height: f64) -> i64 {
    0 // stub — iOS uses UIScene for multi-window
}

#[no_mangle]
pub extern "C" fn perry_ui_window_set_body(_window: i64, _widget: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_show(_window: i64) {}

#[no_mangle]
pub extern "C" fn perry_ui_window_close(_window: i64) {}

// =============================================================================
// LazyVStack
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_create(_count: i64, _render: f64) -> i64 {
    0 // stub
}

#[no_mangle]
pub extern "C" fn perry_ui_lazyvstack_update(_handle: i64, _count: i64) {}

// =============================================================================
// Table (stub — not yet implemented on iOS)
// =============================================================================

#[no_mangle]
pub extern "C" fn perry_ui_table_create(_row_count: f64, _col_count: f64, _render: f64) -> i64 {
    0 // stub
}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_header(_handle: i64, _col: i64, _title_ptr: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_column_width(_handle: i64, _col: i64, _width: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_update_row_count(_handle: i64, _count: i64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_set_on_row_select(_handle: i64, _callback: f64) {}
#[no_mangle]
pub extern "C" fn perry_ui_table_get_selected_row(_handle: i64) -> i64 {
    -1
}
