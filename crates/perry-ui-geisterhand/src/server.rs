//! HTTP server for geisterhand.
//! Routes: /widgets, /click/:handle, /type/:handle, /slide/:handle,
//! /toggle/:handle, /state/:handle, /key, /scroll/:handle, /chaos/start, /chaos/stop, /chaos/status

use tiny_http::{Server, Response, Header, Method};

extern "C" {
    fn perry_geisterhand_get_registry_json(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_free_string(ptr: *mut u8, len: usize);
    fn perry_geisterhand_get_closure(handle: i64, callback_kind: u8) -> f64;
    fn perry_geisterhand_queue_action(closure_f64: f64);
    fn perry_geisterhand_queue_action1(closure_f64: f64, arg: f64);
    fn perry_geisterhand_queue_state_set(handle: i64, value: f64);
    fn perry_geisterhand_request_screenshot(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_find_by_shortcut(shortcut_ptr: *const u8, shortcut_len: usize) -> f64;
    fn perry_geisterhand_queue_scroll(handle: i64, x: f64, y: f64);
    fn perry_geisterhand_queue_set_text(handle: i64, text_ptr: *const u8, text_len: usize);
    fn perry_geisterhand_request_value(handle: i64, out_len: *mut usize) -> *mut u8;
}

// Callback kind constants (must match perry-runtime/src/geisterhand_registry.rs)
const CB_ON_CLICK: u8 = 0;
const CB_ON_CHANGE: u8 = 1;
const CB_ON_SUBMIT: u8 = 2;
const CB_ON_HOVER: u8 = 3;
const CB_ON_DOUBLE_CLICK: u8 = 4;

fn json_header() -> Header {
    Header::from_bytes("Content-Type", "application/json").unwrap()
}

fn cors_header() -> Header {
    Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap()
}

fn ok_json(body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(body.as_bytes().to_vec())
        .with_header(json_header())
        .with_header(cors_header())
}

fn error_json(status: u16, msg: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = format!(r#"{{"error":"{}"}}"#, msg);
    Response::from_data(body.into_bytes())
        .with_status_code(status)
        .with_header(json_header())
        .with_header(cors_header())
}

/// Parse handle from URL path segment (e.g., "/click/3" → 3)
fn parse_handle(path: &str, prefix: &str) -> Option<i64> {
    let rest = path.strip_prefix(prefix)?;
    rest.parse::<i64>().ok()
}

/// Parse a query parameter value from a URL (e.g., "/widgets?label=Save" → Some("Save"))
fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split('?').nth(1)?;
    let needle = format!("{}=", key);
    for pair in query.split('&') {
        if let Some(val) = pair.strip_prefix(&needle) {
            return Some(val);
        }
    }
    None
}

/// Map widget type name to code
fn widget_type_from_name(name: &str) -> Option<u8> {
    match name {
        "button" => Some(0),
        "textfield" | "text_field" => Some(1),
        "slider" => Some(2),
        "toggle" => Some(3),
        "picker" => Some(4),
        "menu" => Some(5),
        "shortcut" => Some(6),
        "table" => Some(7),
        "scrollview" | "scroll_view" => Some(8),
        _ => name.parse::<u8>().ok(),
    }
}

/// Read request body as string
fn read_body(request: &mut tiny_http::Request) -> String {
    let mut body = String::new();
    let _ = request.as_reader().read_to_string(&mut body);
    body
}

pub fn run_server(port: u16) {
    let addr = format!("0.0.0.0:{}", port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[geisterhand] failed to start: {}", e);
            return;
        }
    };

    for mut request in server.incoming_requests() {
        let full_url = request.url().to_string();
        let path = full_url.split('?').next().unwrap_or(&full_url);
        let method = request.method().clone();

        // Handle CORS preflight
        if matches!(method, Method::Options) {
            let resp = Response::from_data(Vec::<u8>::new())
                .with_header(cors_header())
                .with_header(Header::from_bytes("Access-Control-Allow-Methods", "GET, POST, OPTIONS").unwrap())
                .with_header(Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap());
            let _ = request.respond(resp);
            continue;
        }

        let response = match (method, path) {
            // GET /widgets — list all registered widgets (supports ?label= and ?type= filters)
            (Method::Get, "/widgets") => {
                let mut len: usize = 0;
                let ptr = unsafe { perry_geisterhand_get_registry_json(&mut len) };
                let json = if !ptr.is_null() && len > 0 {
                    let s = unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned() };
                    unsafe { perry_geisterhand_free_string(ptr, len); }
                    s
                } else {
                    "[]".to_string()
                };

                // Apply query param filters
                let label_filter = query_param(&full_url, "label");
                let type_filter = query_param(&full_url, "type")
                    .and_then(|t| widget_type_from_name(t));

                if label_filter.is_some() || type_filter.is_some() {
                    if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json) {
                        let filtered: Vec<&serde_json::Value> = arr.iter().filter(|w| {
                            if let Some(label) = label_filter {
                                if let Some(wl) = w.get("label").and_then(|l| l.as_str()) {
                                    if !wl.to_lowercase().contains(&label.to_lowercase()) {
                                        return false;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            if let Some(wt) = type_filter {
                                if let Some(wt_val) = w.get("widget_type").and_then(|t| t.as_u64()) {
                                    if wt_val != wt as u64 {
                                        return false;
                                    }
                                } else {
                                    return false;
                                }
                            }
                            true
                        }).collect();
                        ok_json(&serde_json::to_string(&filtered).unwrap_or_else(|_| "[]".to_string()))
                    } else {
                        ok_json(&json)
                    }
                } else {
                    ok_json(&json)
                }
            }

            // POST /click/:handle — fire onClick
            (Method::Post, p) if p.starts_with("/click/") => {
                match parse_handle(p, "/click/") {
                    Some(handle) => {
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CLICK) };
                        if closure != 0.0 {
                            unsafe { perry_geisterhand_queue_action(closure); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onClick callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /type/:handle — set textfield text + fire onChange
            (Method::Post, p) if p.starts_with("/type/") => {
                match parse_handle(p, "/type/") {
                    Some(handle) => {
                        let body = read_body(&mut request);
                        let text = match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => v.get("text").and_then(|t| t.as_str()).unwrap_or("").to_string(),
                            Err(_) => body.clone(),
                        };
                        // Use SendMessageW directly from this thread (thread-safe Win32 call)
                        // to set the Edit control text, bypassing the action queue.
                        // HWND lookup uses perry-ui-geisterhand's own static (single copy,
                        // avoids the dual perry-runtime static instance issue).
                        let hwnd_val = crate::perry_geisterhand_lookup_hwnd(handle);
                        if hwnd_val != 0 {
                            #[cfg(target_os = "windows")]
                            {
                                // Use raw Win32 FFI — no windows crate dependency needed
                                extern "system" {
                                    fn SendMessageW(hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> isize;
                                }
                                const WM_SETTEXT: u32 = 0x000C;
                                let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
                                unsafe {
                                    SendMessageW(hwnd_val, WM_SETTEXT, 0, wide.as_ptr() as isize);
                                }
                            }
                            // Also fire onChange callback via the action queue
                            let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                            if closure != 0.0 {
                                extern "C" {
                                    fn js_string_from_bytes(ptr: *const u8, len: usize) -> *mut u8;
                                    fn js_nanbox_string(ptr: i64) -> f64;
                                }
                                let text_bytes = text.as_bytes();
                                let str_ptr = unsafe { js_string_from_bytes(text_bytes.as_ptr(), text_bytes.len()) };
                                let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                                unsafe { perry_geisterhand_queue_action1(closure, nanboxed); }
                            }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            // Non-Windows: queue text set via the action queue
                            let text_bytes = text.as_bytes();
                            unsafe { perry_geisterhand_queue_set_text(handle, text_bytes.as_ptr(), text_bytes.len()); }
                            ok_json(r#"{"ok":true}"#)
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /slide/:handle — set slider value + fire onChange
            (Method::Post, p) if p.starts_with("/slide/") => {
                match parse_handle(p, "/slide/") {
                    Some(handle) => {
                        let body = read_body(&mut request);
                        let value = match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => v.get("value").and_then(|v| v.as_f64()).unwrap_or(0.5),
                            Err(_) => 0.5,
                        };
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                        if closure != 0.0 {
                            unsafe { perry_geisterhand_queue_action1(closure, value); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onChange callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /toggle/:handle — toggle + fire onChange
            (Method::Post, p) if p.starts_with("/toggle/") => {
                match parse_handle(p, "/toggle/") {
                    Some(handle) => {
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                        if closure != 0.0 {
                            // Toggle with TAG_TRUE (0x7FFC_0000_0000_0004)
                            let tag_true = f64::from_bits(0x7FFC_0000_0000_0004u64);
                            unsafe { perry_geisterhand_queue_action1(closure, tag_true); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onChange callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /state/:handle — set state value
            (Method::Post, p) if p.starts_with("/state/") => {
                match parse_handle(p, "/state/") {
                    Some(handle) => {
                        let body = read_body(&mut request);
                        let value = match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => v.get("value").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            Err(_) => 0.0,
                        };
                        unsafe { perry_geisterhand_queue_state_set(handle, value); }
                        ok_json(r#"{"ok":true}"#)
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /hover/:handle — fire onHover
            (Method::Post, p) if p.starts_with("/hover/") => {
                match parse_handle(p, "/hover/") {
                    Some(handle) => {
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_HOVER) };
                        if closure != 0.0 {
                            unsafe { perry_geisterhand_queue_action(closure); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onHover callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /doubleclick/:handle — fire onDoubleClick
            (Method::Post, p) if p.starts_with("/doubleclick/") => {
                match parse_handle(p, "/doubleclick/") {
                    Some(handle) => {
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_DOUBLE_CLICK) };
                        if closure != 0.0 {
                            unsafe { perry_geisterhand_queue_action(closure); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onDoubleClick callback for this handle")
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /chaos/start — start chaos mode
            (Method::Post, "/chaos/start") => {
                let body = read_body(&mut request);
                let interval_ms = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v.get("interval_ms").and_then(|v| v.as_u64()).unwrap_or(100) as u64,
                    Err(_) => 100,
                };
                let seed = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v.get("seed").and_then(|v| v.as_u64()),
                    Err(_) => None,
                };
                crate::chaos::start(interval_ms, seed);
                ok_json(r#"{"ok":true,"chaos":"started"}"#)
            }

            // POST /chaos/stop — stop chaos mode
            (Method::Post, "/chaos/stop") => {
                crate::chaos::stop();
                ok_json(r#"{"ok":true,"chaos":"stopped"}"#)
            }

            // GET /chaos/status — chaos mode stats
            (Method::Get, "/chaos/status") => {
                let status = crate::chaos::status();
                ok_json(&status)
            }

            // GET /health
            (Method::Get, "/health") => {
                ok_json(r#"{"status":"ok"}"#)
            }

            // GET /screenshot — capture the app window as PNG
            (Method::Get, "/screenshot") => {
                let mut len: usize = 0;
                let ptr = unsafe { perry_geisterhand_request_screenshot(&mut len) };
                if !ptr.is_null() && len > 0 {
                    let data = unsafe { std::slice::from_raw_parts(ptr, len).to_vec() };
                    unsafe { perry_geisterhand_free_string(ptr, len); }
                    Response::from_data(data)
                        .with_header(Header::from_bytes("Content-Type", "image/png").unwrap())
                        .with_header(cors_header())
                } else {
                    error_json(500, "screenshot capture failed or timed out")
                }
            }

            // POST /key — fire a keyboard shortcut by matching registered menu shortcuts
            (Method::Post, "/key") => {
                let body = read_body(&mut request);
                let shortcut = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => v.get("shortcut").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                    Err(_) => body.trim().to_string(),
                };
                if shortcut.is_empty() {
                    error_json(400, "missing shortcut field")
                } else {
                    let closure = unsafe {
                        perry_geisterhand_find_by_shortcut(shortcut.as_ptr(), shortcut.len())
                    };
                    if closure != 0.0 {
                        unsafe { perry_geisterhand_queue_action(closure); }
                        ok_json(r#"{"ok":true}"#)
                    } else {
                        error_json(404, "no registered shortcut matches")
                    }
                }
            }

            // POST /scroll/:handle — scroll a scrollview
            (Method::Post, p) if p.starts_with("/scroll/") => {
                match parse_handle(p, "/scroll/") {
                    Some(handle) => {
                        let body = read_body(&mut request);
                        let (x, y) = match serde_json::from_str::<serde_json::Value>(&body) {
                            Ok(v) => (
                                v.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0),
                                v.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0),
                            ),
                            Err(_) => (0.0, 0.0),
                        };
                        unsafe { perry_geisterhand_queue_scroll(handle, x, y); }
                        ok_json(r#"{"ok":true}"#)
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            // POST /wait — wait for a widget with matching label to appear
            (Method::Post, "/wait") => {
                let body = read_body(&mut request);
                let (label, timeout_ms) = match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => (
                        v.get("label").and_then(|s| s.as_str()).unwrap_or("").to_string(),
                        v.get("timeout").and_then(|t| t.as_u64()).unwrap_or(5000),
                    ),
                    Err(_) => (String::new(), 5000),
                };
                if label.is_empty() {
                    error_json(400, "missing label field")
                } else {
                    let start = std::time::Instant::now();
                    let timeout = std::time::Duration::from_millis(timeout_ms);
                    loop {
                        let mut len: usize = 0;
                        let ptr = unsafe { perry_geisterhand_get_registry_json(&mut len) };
                        if !ptr.is_null() && len > 0 {
                            let json = unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned() };
                            unsafe { perry_geisterhand_free_string(ptr, len); }
                            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(&json) {
                                if let Some(w) = arr.iter().find(|w| {
                                    w.get("label").and_then(|l| l.as_str())
                                        .map(|l| l.to_lowercase().contains(&label.to_lowercase()))
                                        .unwrap_or(false)
                                }) {
                                    break ok_json(&serde_json::to_string(w).unwrap_or_else(|_| "{}".to_string()));
                                }
                            }
                        }
                        if start.elapsed() >= timeout {
                            break error_json(408, "timeout waiting for widget");
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                }
            }

            // GET /value/:handle — read current widget value
            (Method::Get, p) if p.starts_with("/value/") => {
                match parse_handle(p, "/value/") {
                    Some(handle) => {
                        let mut len: usize = 0;
                        let ptr = unsafe { perry_geisterhand_request_value(handle, &mut len) };
                        if !ptr.is_null() && len > 0 {
                            let val = unsafe { String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len)).into_owned() };
                            unsafe { perry_geisterhand_free_string(ptr, len); }
                            ok_json(&format!(r#"{{"handle":{},"value":"{}"}}"#, handle, val.replace('"', "\\\"")))
                        } else {
                            ok_json(&format!(r#"{{"handle":{},"value":null}}"#, handle))
                        }
                    }
                    None => error_json(400, "invalid handle"),
                }
            }

            _ => error_json(404, "not found"),
        };

        let _ = request.respond(response);
    }
}
