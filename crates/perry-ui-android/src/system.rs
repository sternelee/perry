//! System APIs — open_url, dark mode, preferences, keychain, notifications

use jni::objects::JValue;
use crate::jni_bridge;

fn str_from_header(ptr: *const u8) -> &'static str {
    crate::app::str_from_header(ptr)
}

extern "C" {
    fn js_string_from_bytes(ptr: *const u8, len: usize) -> *const u8;
    fn js_nanbox_string(ptr: *const u8) -> f64;
}

/// Open a URL in the default browser via Intent.ACTION_VIEW.
pub fn open_url(url_ptr: *const u8) {
    let url = str_from_header(url_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(32);

    let activity = crate::widgets::get_activity(&mut env);

    let jurl = env.new_string(url).expect("Failed to create JNI string");
    let uri = env.call_static_method(
        "android/net/Uri",
        "parse",
        "(Ljava/lang/String;)Landroid/net/Uri;",
        &[JValue::Object(&jurl)],
    ).expect("Uri.parse").l().expect("uri");

    let action = env.new_string("android.intent.action.VIEW").expect("action string");
    let intent = env.new_object(
        "android/content/Intent",
        "(Ljava/lang/String;Landroid/net/Uri;)V",
        &[JValue::Object(&action), JValue::Object(&uri)],
    ).expect("Intent");

    let _ = env.call_method(
        &activity,
        "startActivity",
        "(Landroid/content/Intent;)V",
        &[JValue::Object(&intent)],
    );

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}

/// Check if dark mode is enabled via Configuration.uiMode.
pub fn is_dark_mode() -> i64 {
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);

    let resources = env.call_method(
        &activity,
        "getResources",
        "()Landroid/content/res/Resources;",
        &[],
    ).expect("getResources").l().expect("resources");

    let config = env.call_method(
        &resources,
        "getConfiguration",
        "()Landroid/content/res/Configuration;",
        &[],
    ).expect("getConfiguration").l().expect("configuration");

    let ui_mode = env.get_field(&config, "uiMode", "I")
        .expect("uiMode").i().expect("int");

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }

    // UI_MODE_NIGHT_MASK = 0x30, UI_MODE_NIGHT_YES = 0x20
    if (ui_mode & 0x30) == 0x20 { 1 } else { 0 }
}

/// Set a preference value using SharedPreferences.
pub fn preferences_set(key_ptr: *const u8, value: f64) {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_prefs").expect("pref name");
    let prefs = env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)], // MODE_PRIVATE = 0
    ).expect("getSharedPreferences").l().expect("prefs");

    let editor = env.call_method(
        &prefs,
        "edit",
        "()Landroid/content/SharedPreferences$Editor;",
        &[],
    ).expect("edit").l().expect("editor");

    let jkey = env.new_string(key).expect("key string");

    // Check if value is a NaN-boxed string
    let bits = value.to_bits();
    let tag = (bits >> 48) as u16;
    if tag == 0x7FFF {
        // String value — extract and store as string
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const u8;
        let s = str_from_header(ptr);
        let jval = env.new_string(s).expect("value string");
        let _ = env.call_method(
            &editor,
            "putString",
            "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&jkey), JValue::Object(&jval)],
        );
    } else {
        // Numeric value
        let _ = env.call_method(
            &editor,
            "putFloat",
            "(Ljava/lang/String;F)Landroid/content/SharedPreferences$Editor;",
            &[JValue::Object(&jkey), JValue::Float(value as f32)],
        );
    }

    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}

/// Get a preference value from SharedPreferences.
pub fn preferences_get(key_ptr: *const u8) -> f64 {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_prefs").expect("pref name");
    let prefs = env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)],
    ).expect("getSharedPreferences").l().expect("prefs");

    let jkey = env.new_string(key).expect("key string");

    // Try getString first, fallback to getFloat
    let str_result = env.call_method(
        &prefs,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[JValue::Object(&jkey), JValue::Object(&jni::objects::JObject::null())],
    );

    if let Ok(val) = str_result {
        if let Ok(obj) = val.l() {
            if !obj.is_null() {
                let jstr: jni::objects::JString = obj.into();
                let s: String = env.get_string(&jstr).expect("get string").into();
                let bytes = s.as_bytes();
                let ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
                let result = unsafe { js_nanbox_string(ptr) };
                unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
                return result;
            }
        }
    }

    // Try getFloat
    let float_result = env.call_method(
        &prefs,
        "getFloat",
        "(Ljava/lang/String;F)F",
        &[JValue::Object(&jkey), JValue::Float(0.0)],
    );

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }

    if let Ok(val) = float_result {
        if let Ok(f) = val.f() {
            return f as f64;
        }
    }

    0.0
}

/// Save a value to the keychain (SharedPreferences with private mode).
pub fn keychain_save(key_ptr: *const u8, value_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    let value = str_from_header(value_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)],
    ).expect("getSharedPreferences").l().expect("prefs");

    let editor = env.call_method(
        &prefs,
        "edit",
        "()Landroid/content/SharedPreferences$Editor;",
        &[],
    ).expect("edit").l().expect("editor");

    let jkey = env.new_string(key).expect("key string");
    let jval = env.new_string(value).expect("value string");
    let _ = env.call_method(
        &editor,
        "putString",
        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        &[JValue::Object(&jkey), JValue::Object(&jval)],
    );
    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}

/// Get a value from the keychain.
pub fn keychain_get(key_ptr: *const u8) -> f64 {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)],
    ).expect("getSharedPreferences").l().expect("prefs");

    let jkey = env.new_string(key).expect("key string");
    let result = env.call_method(
        &prefs,
        "getString",
        "(Ljava/lang/String;Ljava/lang/String;)Ljava/lang/String;",
        &[JValue::Object(&jkey), JValue::Object(&jni::objects::JObject::null())],
    );

    let mut ret_val = f64::from_bits(0x7FFC_0000_0000_0001u64); // undefined
    if let Ok(val) = result {
        if let Ok(obj) = val.l() {
            if !obj.is_null() {
                let jstr: jni::objects::JString = obj.into();
                let s: String = env.get_string(&jstr).expect("get string").into();
                let bytes = s.as_bytes();
                let ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len()) };
                ret_val = unsafe { js_nanbox_string(ptr) };
            }
        }
    }

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
    ret_val
}

/// Delete a value from the keychain.
pub fn keychain_delete(key_ptr: *const u8) {
    let key = str_from_header(key_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let pref_name = env.new_string("perry_keychain").expect("pref name");
    let prefs = env.call_method(
        &activity,
        "getSharedPreferences",
        "(Ljava/lang/String;I)Landroid/content/SharedPreferences;",
        &[JValue::Object(&pref_name), JValue::Int(0)],
    ).expect("getSharedPreferences").l().expect("prefs");

    let editor = env.call_method(
        &prefs,
        "edit",
        "()Landroid/content/SharedPreferences$Editor;",
        &[],
    ).expect("edit").l().expect("editor");

    let jkey = env.new_string(key).expect("key string");
    let _ = env.call_method(
        &editor,
        "remove",
        "(Ljava/lang/String;)Landroid/content/SharedPreferences$Editor;",
        &[JValue::Object(&jkey)],
    );
    let _ = env.call_method(&editor, "apply", "()V", &[]);

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}

/// Send a notification via PerryBridge.
pub fn notification_send(title_ptr: *const u8, body_ptr: *const u8) {
    let title = str_from_header(title_ptr);
    let body = str_from_header(body_ptr);
    let mut env = jni_bridge::get_env();
    let _ = env.push_local_frame(16);

    let activity = crate::widgets::get_activity(&mut env);
    let bridge_class = jni_bridge::with_cache(|c| {
        env.new_local_ref(c.perry_bridge_class.as_obj()).unwrap()
    });

    let jtitle = env.new_string(title).expect("title string");
    let jbody = env.new_string(body).expect("body string");

    let bridge_cls: &jni::objects::JClass = (&bridge_class).into();
    let _ = env.call_static_method(
        bridge_cls,
        "sendNotification",
        "(Landroid/app/Activity;Ljava/lang/String;Ljava/lang/String;)V",
        &[
            JValue::Object(&activity),
            JValue::Object(&jtitle),
            JValue::Object(&jbody),
        ],
    );

    unsafe { env.pop_local_frame(&jni::objects::JObject::null()); }
}
