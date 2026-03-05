use jni::objects::JValue;
use crate::app::str_from_header;
use crate::callback;
use crate::jni_bridge;

extern "C" {
    fn __android_log_print(prio: i32, tag: *const u8, fmt: *const u8, ...) -> i32;
}

/// Create a Button with a label and closure callback. Returns widget handle.
pub fn create(label_ptr: *const u8, on_press: f64) -> i64 {
    let label = str_from_header(label_ptr);
    unsafe {
        __android_log_print(3, b"PerryButton\0".as_ptr(), b"create: label='%s'\0".as_ptr(), label.as_ptr());
    }
    let mut env = jni_bridge::get_env();

    // Check for pending exception from prior JNI calls
    if env.exception_check().unwrap_or(false) {
        unsafe {
            __android_log_print(6, b"PerryButton\0".as_ptr(), b"create: PENDING EXCEPTION before get_activity!\0".as_ptr());
        }
        let _ = env.exception_describe();
        let _ = env.exception_clear();
    }

    // Ensure we have local ref space
    let _ = env.push_local_frame(32);

    let activity = super::get_activity(&mut env);
    unsafe {
        __android_log_print(3, b"PerryButton\0".as_ptr(), b"create: got activity, using cached constructor\0".as_ptr());
    }

    // Use cached class and constructor to avoid FindClass overhead
    let button_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.button_class.as_obj()).unwrap()
    });
    let ctor_id = jni_bridge::with_cache(|c| c.button_init);
    let button_cls: &jni::objects::JClass = (&button_class).into();

    unsafe {
        __android_log_print(3, b"PerryButton\0".as_ptr(), b"create: calling NewObject with cached ctor\0".as_ptr());
    }

    let button = match unsafe {
        env.new_object_unchecked(
            button_cls,
            ctor_id,
            &[JValue::Object(&activity).as_jni()],
        )
    } {
        Ok(b) => b,
        Err(e) => {
            let msg = format!("Failed to create Button: {:?}\0", e);
            unsafe {
                __android_log_print(6, b"PerryButton\0".as_ptr(), b"create: FAILED: %s\0".as_ptr(), msg.as_ptr());
            }
            if env.exception_check().unwrap_or(false) {
                let _ = env.exception_describe();
                let _ = env.exception_clear();
            }
            unsafe { let _ = env.pop_local_frame(&jni::objects::JObject::null()); }
            panic!("Failed to create Button: {:?}", e);
        }
    };
    unsafe {
        __android_log_print(3, b"PerryButton\0".as_ptr(), b"create: Button created OK\0".as_ptr());
    }

    // Set label text
    let jstr = env.new_string(label).expect("Failed to create JNI string");
    let _ = env.call_method(
        &button,
        "setText",
        "(Ljava/lang/CharSequence;)V",
        &[JValue::Object(&jstr)],
    );

    // Disable ALL CAPS (Material default) to match iOS mixed-case behavior
    let _ = env.call_method(
        &button,
        "setAllCaps",
        "(Z)V",
        &[JValue::Bool(0)],
    );

    // Register callback and set up OnClickListener via PerryBridge
    let cb_key = callback::register(on_press);
    let bridge_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
    });
    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "setOnClickCallback",
        "(Landroid/view/View;J)V",
        &[JValue::Object(&button), JValue::Long(cb_key)],
    );

    let global = env.new_global_ref(button).expect("Failed to create global ref");
    let handle = super::register_widget(global);
    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
    handle
}

/// Set whether a button has a border.
/// On Android, buttons always have a background; toggle between Material styles.
pub fn set_bordered(handle: i64, bordered: bool) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        if !bordered {
            // Set a flat/borderless style by making background transparent
            let bridge_class = jni_bridge::with_cache(|c| {
                env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
            });
            let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
            let _ = env.call_static_method(
                bridge_cls,
                "setButtonBorderless",
                "(Landroid/view/View;Z)V",
                &[JValue::Object(view_ref.as_obj()), JValue::Bool(0)],
            );
        }
        unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
    }
}

/// Set the text color of a button.
pub fn set_text_color(handle: i64, r: f64, g: f64, b: f64, a: f64) {
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let ai = (a * 255.0) as u32;
        let ri = (r * 255.0) as u32;
        let gi = (g * 255.0) as u32;
        let bi = (b * 255.0) as u32;
        let color = ((ai << 24) | (ri << 16) | (gi << 8) | bi) as i32;
        let _ = env.call_method(
            view_ref.as_obj(),
            "setTextColor",
            "(I)V",
            &[JValue::Int(color)],
        );
        unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
    }
}

/// Set the title text of a button.
pub fn set_title(handle: i64, title_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    if let Some(view_ref) = super::get_widget(handle) {
        let mut env = jni_bridge::get_env();
        let _ = env.push_local_frame(8);
        let jstr = env.new_string(title).expect("Failed to create JNI string");
        let _ = env.call_method(
            view_ref.as_obj(),
            "setText",
            "(Ljava/lang/CharSequence;)V",
            &[JValue::Object(&jstr)],
        );
        unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
    }
}
