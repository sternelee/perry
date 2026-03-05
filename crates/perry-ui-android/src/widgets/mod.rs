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
pub mod tabbar;

use jni::objects::{GlobalRef, JObject, JValue};
use std::sync::Mutex;

use crate::jni_bridge;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Global widget registry — shared across threads so widgets created on
/// the native thread can be accessed from UI-thread callbacks.
static WIDGETS: Mutex<Vec<GlobalRef>> = Mutex::new(Vec::new());

/// Store an Android View and return its handle (1-based i64).
pub fn register_widget(view: GlobalRef) -> i64 {
    let mut widgets = WIDGETS.lock().unwrap();
    widgets.push(view);
    widgets.len() as i64
}

/// Retrieve the JNI GlobalRef for a given widget handle.
pub fn get_widget(handle: i64) -> Option<GlobalRef> {
    let widgets = WIDGETS.lock().unwrap();
    let idx = (handle - 1) as usize;
    widgets.get(idx).cloned()
}

/// Set the hidden state of a widget (View.VISIBLE=0, View.GONE=8).
pub fn set_hidden(handle: i64, hidden: bool) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let visibility = if hidden { 8i32 } else { 0i32 }; // View.GONE=8, View.VISIBLE=0
        let _ = env.call_method(
            view_ref.as_obj(),
            "setVisibility",
            "(I)V",
            &[JValue::Int(visibility)],
        );
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Remove all child views from a ViewGroup container.
/// When clearing the root widget, also releases global refs for all child widgets
/// to prevent JNI global reference table overflow on rebuilds.
pub fn clear_children(handle: i64) {
    // Track the first widget that gets clearChildren called — it's the root
    crate::app::track_root_candidate(handle);
    if let Some(parent_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            parent_ref.as_obj(),
            "removeAllViews",
            "()V",
            &[],
        );
        unsafe {
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            env.pop_local_frame(&JObject::null());
        }

        // Release global refs for widgets created after this handle.
        // Perry rebuilds the entire UI tree on each rebuild(), so all
        // widgets after the root are temporary and get recreated.
        let idx = (handle - 1) as usize;
        let mut widgets = WIDGETS.lock().unwrap();
        if idx < widgets.len() {
            // Drop all global refs after this handle (they are removed from the view tree)
            widgets.truncate(idx + 1);
        }
    }
}

/// Add a child view to a parent ViewGroup.
/// For vertical LinearLayout parents (VStack), sets child width to MATCH_PARENT
/// to match iOS UIStackView fill alignment behavior.
pub fn add_child(parent_handle: i64, child_handle: i64) {
    if let (Some(parent_ref), Some(child_ref)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let result = env.call_method(
            parent_ref.as_obj(),
            "addView",
            "(Landroid/view/View;)V",
            &[JValue::Object(child_ref.as_obj())],
        );

        // Match iOS UIStackView fill alignment: adjust child LayoutParams
        // based on parent LinearLayout orientation
        if result.is_ok() {
            if env.is_instance_of(parent_ref.as_obj(), "android/widget/LinearLayout").unwrap_or(false) {
                let orientation = env.call_method(parent_ref.as_obj(), "getOrientation", "()I", &[])
                    .map(|v| v.i().unwrap_or(-1)).unwrap_or(-1);
                if let Ok(lp) = env.call_method(child_ref.as_obj(), "getLayoutParams",
                    "()Landroid/view/ViewGroup$LayoutParams;", &[]) {
                    if let Ok(lp_obj) = lp.l() {
                        if !lp_obj.is_null() {
                            if orientation == 1 { // VERTICAL — stretch children to fill width
                                let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(-1)); // MATCH_PARENT
                            } else if orientation == 0 { // HORIZONTAL — share space equally
                                // If child has MATCH_PARENT width, convert to weight-based
                                // so multiple children share the HStack evenly
                                let cur_w = env.get_field(&lp_obj, "width", "I")
                                    .map(|v| v.i().unwrap_or(0)).unwrap_or(0);
                                if cur_w == -1 { // MATCH_PARENT
                                    let _ = env.set_field(&lp_obj, "width", "I", JValue::Int(0));
                                    if env.is_instance_of(&lp_obj, "android/widget/LinearLayout$LayoutParams").unwrap_or(false) {
                                        let _ = env.set_field(&lp_obj, "weight", "F", JValue::Float(1.0));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        unsafe {
            if env.exception_check().unwrap_or(false) {
                __android_log_print(
                    6, b"PerryWidgets\0".as_ptr(),
                    b"add_child: JNI EXCEPTION!\0".as_ptr(),
                );
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            env.pop_local_frame(&JObject::null());
        }
    }
}

/// Add a child view to a parent ViewGroup at a specific index.
pub fn add_child_at(parent_handle: i64, child_handle: i64, index: i64) {
    if let (Some(parent_ref), Some(child_ref)) = (get_widget(parent_handle), get_widget(child_handle)) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            parent_ref.as_obj(),
            "addView",
            "(Landroid/view/View;I)V",
            &[JValue::Object(child_ref.as_obj()), JValue::Int(index as i32)],
        );
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Get the Activity context via PerryBridge.
pub fn get_activity<'a>(env: &mut jni::JNIEnv<'a>) -> JObject<'a> {
    let bridge_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
    });
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "getActivity",
        "()Landroid/app/Activity;",
        &[],
    ).expect("Failed to get Activity");
    result.l().expect("Activity is not an object")
}

/// Set enabled/disabled on a widget.
pub fn set_enabled(handle: i64, enabled: bool) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let _ = env.call_method(
            view_ref.as_obj(),
            "setEnabled",
            "(Z)V",
            &[JValue::Bool(enabled as u8)],
        );
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set tooltip (API 26+).
pub fn set_tooltip(handle: i64, text_ptr: *const u8) {
    let text = crate::app::str_from_header(text_ptr);
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(text).expect("tooltip string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTooltipText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set control size (map to scale).
pub fn set_control_size(handle: i64, size: i64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let scale = match size {
            0 => 0.75f32,  // mini
            1 => 0.85f32,  // small
            3 => 1.15f32,  // large
            _ => 1.0f32,   // regular
        };
        let _ = env.call_method(view_ref.as_obj(), "setScaleX", "(F)V", &[JValue::Float(scale)]);
        let _ = env.call_method(view_ref.as_obj(), "setScaleY", "(F)V", &[JValue::Float(scale)]);
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set corner radius via GradientDrawable.
/// If the view already has a GradientDrawable background, updates its corner radius
/// (preserving the existing color). Otherwise creates a new transparent GradientDrawable.
pub fn set_corner_radius(handle: i64, radius: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let radius_px = dp_to_px(&mut env, radius as f32) as f32;

        // Try to reuse existing GradientDrawable background (preserving color)
        let mut reused = false;
        if let Ok(bg) = env.call_method(view_ref.as_obj(), "getBackground",
            "()Landroid/graphics/drawable/Drawable;", &[])
        {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env.is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable").unwrap_or(false) {
                        let _ = env.call_method(&bg_obj, "setCornerRadius", "(F)V",
                            &[JValue::Float(radius_px)]);
                        reused = true;
                    }
                }
            }
        }
        if !reused {
            let gd = env.new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
                .expect("GradientDrawable");
            let _ = env.call_method(&gd, "setCornerRadius", "(F)V", &[JValue::Float(radius_px)]);
            let _ = env.call_method(&gd, "setColor", "(I)V", &[JValue::Int(0)]);
            let _ = env.call_method(
                view_ref.as_obj(),
                "setBackground",
                "(Landroid/graphics/drawable/Drawable;)V",
                &[JValue::Object(&gd)],
            );
        }
        let _ = env.call_method(view_ref.as_obj(), "setClipToOutline", "(Z)V", &[JValue::Bool(1)]);
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set background color using GradientDrawable for compatibility with corner radius.
/// If the view already has a GradientDrawable, updates its color (preserving corner radius).
pub fn set_background_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        let color = argb_color(a, r, g, b);

        // Try to reuse existing GradientDrawable (preserving corner radius)
        let mut reused = false;
        if let Ok(bg) = env.call_method(view_ref.as_obj(), "getBackground",
            "()Landroid/graphics/drawable/Drawable;", &[])
        {
            if let Ok(bg_obj) = bg.l() {
                if !bg_obj.is_null() {
                    if env.is_instance_of(&bg_obj, "android/graphics/drawable/GradientDrawable").unwrap_or(false) {
                        let _ = env.call_method(&bg_obj, "setColor", "(I)V", &[JValue::Int(color)]);
                        reused = true;
                    }
                }
            }
        }
        if !reused {
            // Create GradientDrawable so a later set_corner_radius can reuse it
            let gd = env.new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
                .expect("GradientDrawable");
            let _ = env.call_method(&gd, "setColor", "(I)V", &[JValue::Int(color)]);
            let _ = env.call_method(
                view_ref.as_obj(),
                "setBackground",
                "(Landroid/graphics/drawable/Drawable;)V",
                &[JValue::Object(&gd)],
            );
        }
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set background gradient.
pub fn set_background_gradient(handle: i64, r1: f64, g1: f64, b1: f64, a1: f64, r2: f64, g2: f64, b2: f64, a2: f64, direction: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);

        let c1 = argb_color(a1, r1, g1, b1);
        let c2 = argb_color(a2, r2, g2, b2);

        let gd = env.new_object("android/graphics/drawable/GradientDrawable", "()V", &[])
            .expect("GradientDrawable");

        // Set colors
        let colors = env.new_int_array(2).expect("int array");
        let _ = env.set_int_array_region(&colors, 0, &[c1, c2]);
        let _ = env.call_method(
            &gd,
            "setColors",
            "([I)V",
            &[JValue::Object(&colors)],
        );

        // Set orientation
        let orient_name = if direction < 0.5 { "TOP_BOTTOM" } else { "LEFT_RIGHT" };
        let orient_class = env.find_class("android/graphics/drawable/GradientDrawable$Orientation")
            .expect("Orientation");
        let orient = env.get_static_field(
            &orient_class,
            orient_name,
            "Landroid/graphics/drawable/GradientDrawable$Orientation;",
        ).expect("orient").l().expect("orient obj");
        let _ = env.call_method(
            &gd,
            "setOrientation",
            "(Landroid/graphics/drawable/GradientDrawable$Orientation;)V",
            &[JValue::Object(&orient)],
        );

        let _ = env.call_method(
            view_ref.as_obj(),
            "setBackground",
            "(Landroid/graphics/drawable/Drawable;)V",
            &[JValue::Object(&gd)],
        );

        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set on-hover callback (no-op on Android touch devices).
pub fn set_on_hover(_handle: i64, _callback: f64) {
    // No-op — hover events are uncommon on touch devices
}

/// Set double-click (double-tap) callback.
pub fn set_on_double_click(_handle: i64, _callback: f64) {
    // Would require GestureDetector setup via PerryBridge
    // No-op for now
}

/// Animate opacity.
pub fn animate_opacity(handle: i64, target: f64, duration_ms: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let animator = env.call_method(view_ref.as_obj(), "animate", "()Landroid/view/ViewPropertyAnimator;", &[])
            .expect("animate").l().expect("animator");
        let _ = env.call_method(&animator, "alpha", "(F)Landroid/view/ViewPropertyAnimator;", &[JValue::Float(target as f32)]);
        let _ = env.call_method(&animator, "setDuration", "(J)Landroid/view/ViewPropertyAnimator;", &[JValue::Long(duration_ms as i64)]);
        let _ = env.call_method(&animator, "start", "()V", &[]);
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Animate position.
pub fn animate_position(handle: i64, dx: f64, dy: f64, duration_ms: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let animator = env.call_method(view_ref.as_obj(), "animate", "()Landroid/view/ViewPropertyAnimator;", &[])
            .expect("animate").l().expect("animator");
        let _ = env.call_method(&animator, "translationXBy", "(F)Landroid/view/ViewPropertyAnimator;", &[JValue::Float(dx as f32)]);
        let _ = env.call_method(&animator, "translationYBy", "(F)Landroid/view/ViewPropertyAnimator;", &[JValue::Float(dy as f32)]);
        let _ = env.call_method(&animator, "setDuration", "(J)Landroid/view/ViewPropertyAnimator;", &[JValue::Long(duration_ms as i64)]);
        let _ = env.call_method(&animator, "start", "()V", &[]);
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set on-click callback for any widget (via PerryBridge).
pub fn set_on_click(handle: i64, callback: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let cb_key = crate::callback::register(callback);
        let bridge_class = jni_bridge::with_cache(|c| {
            env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
        });
        let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
        let _ = env.call_static_method(
            bridge_cls,
            "setOnClickCallback",
            "(Landroid/view/View;J)V",
            &[JValue::Object(view_ref.as_obj()), JValue::Long(cb_key)],
        );
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

/// Set content hugging priority (layout weight hint).
/// On Android, this maps to LinearLayout.LayoutParams.weight.
/// A low hugging value means the view WANTS to expand (high weight).
pub fn set_hugging(handle: i64, priority: f64) {
    if let Some(view_ref) = get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(16);
        // Map hugging priority to weight: low hugging = high weight (expands more)
        let weight = if priority < 100.0 { 1.0f32 } else { 0.0f32 };
        // Create LinearLayout.LayoutParams(MATCH_PARENT, 0, weight) for vertical expansion
        let params = env.new_object(
            "android/widget/LinearLayout$LayoutParams",
            "(IIF)V",
            &[JValue::Int(-1), JValue::Int(0), JValue::Float(weight)],
        );
        if let Ok(params) = params {
            let _ = env.call_method(
                view_ref.as_obj(),
                "setLayoutParams",
                "(Landroid/view/ViewGroup$LayoutParams;)V",
                &[JValue::Object(&params)],
            );
        }
        unsafe { env.pop_local_frame(&JObject::null()); }
    }
}

fn argb_color(a: f64, r: f64, g: f64, b: f64) -> i32 {
    let ai = (a * 255.0) as u32;
    let ri = (r * 255.0) as u32;
    let gi = (g * 255.0) as u32;
    let bi = (b * 255.0) as u32;
    ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32
}

/// Convert dp to pixels via PerryBridge.
pub fn dp_to_px(env: &mut jni::JNIEnv, dp: f32) -> i32 {
    let bridge_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
    });
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let result = env.call_static_method(
        bridge_cls,
        "dpToPx",
        "(F)I",
        &[JValue::Float(dp)],
    ).expect("Failed to convert dp to px");
    result.i().expect("dpToPx did not return int")
}
