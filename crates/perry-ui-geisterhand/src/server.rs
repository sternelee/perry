//! HTTP server for geisterhand.
//! Routes: /widgets, /click/:handle, /type/:handle, /slide/:handle,
//! /toggle/:handle, /state/:handle, /key, /chaos/start, /chaos/stop, /chaos/status

use tiny_http::{Server, Response, Header, Method};

extern "C" {
    fn perry_geisterhand_get_registry_json(out_len: *mut usize) -> *mut u8;
    fn perry_geisterhand_free_string(ptr: *mut u8, len: usize);
    fn perry_geisterhand_get_closure(handle: i64, callback_kind: u8) -> f64;
    fn perry_geisterhand_queue_action(closure_f64: f64);
    fn perry_geisterhand_queue_action1(closure_f64: f64, arg: f64);
    fn perry_geisterhand_queue_state_set(handle: i64, value: f64);
    fn perry_geisterhand_request_screenshot(out_len: *mut usize) -> *mut u8;
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
        let path = request.url().to_string();
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

        let response = match (method, path.as_str()) {
            // GET /widgets — list all registered widgets
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
                ok_json(&json)
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
                            Err(_) => String::new(),
                        };
                        let closure = unsafe { perry_geisterhand_get_closure(handle, CB_ON_CHANGE) };
                        if closure != 0.0 {
                            // Create a NaN-boxed string from the text
                            // For now, pass the closure_f64 as-is — the text value
                            // needs NaN-boxing which requires calling into the runtime
                            extern "C" {
                                fn js_string_from_bytes(ptr: *const u8, len: usize) -> *mut u8;
                                fn js_nanbox_string(ptr: i64) -> f64;
                            }
                            let text_bytes = text.as_bytes();
                            let str_ptr = unsafe { js_string_from_bytes(text_bytes.as_ptr(), text_bytes.len()) };
                            let nanboxed = unsafe { js_nanbox_string(str_ptr as i64) };
                            unsafe { perry_geisterhand_queue_action1(closure, nanboxed); }
                            ok_json(r#"{"ok":true}"#)
                        } else {
                            error_json(404, "no onChange callback for this handle")
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

            _ => error_json(404, "not found"),
        };

        let _ = request.respond(response);
    }
}
