//! Date operations runtime support
//!
//! Provides JavaScript Date functionality using system time.
//! Dates are represented internally as i64 timestamps (milliseconds since Unix epoch).

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current timestamp in milliseconds (Date.now())
#[no_mangle]
pub extern "C" fn js_date_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as f64)
        .unwrap_or(0.0)
}

/// performance.now() — high-resolution time in milliseconds (sub-ms precision).
/// Returns ms since UNIX_EPOCH as f64; the float retains microsecond resolution.
#[no_mangle]
pub extern "C" fn js_performance_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Create a new Date from current time, returning timestamp in milliseconds
#[no_mangle]
pub extern "C" fn js_date_new() -> f64 {
    js_date_now()
}

/// Create a new Date from a timestamp (milliseconds since epoch)
#[no_mangle]
pub extern "C" fn js_date_new_from_timestamp(timestamp: f64) -> f64 {
    timestamp
}

/// Create a new Date from a value that could be a number or a NaN-boxed string.
/// Checks for STRING_TAG (0x7FFF) in the top 16 bits; if found, parses the string
/// as a date. Otherwise treats the value as a numeric timestamp.
#[no_mangle]
pub extern "C" fn js_date_new_from_value(value: f64) -> f64 {
    let bits = value.to_bits();
    let tag = (bits >> 48) & 0xFFFF;
    if tag == 0x7FFF {
        // NaN-boxed string — extract pointer and parse
        let ptr = (bits & 0x0000_FFFF_FFFF_FFFF) as *const crate::StringHeader;
        if ptr.is_null() || (ptr as usize) < 0x1000 {
            return f64::NAN;
        }
        unsafe {
            let len = (*ptr).length as usize;
            let data = (ptr as *const u8).add(std::mem::size_of::<crate::StringHeader>());
            let bytes = std::slice::from_raw_parts(data, len);
            if let Ok(s) = std::str::from_utf8(bytes) {
                parse_date_string(s)
            } else {
                f64::NAN
            }
        }
    } else {
        // Numeric timestamp
        value
    }
}

/// Parse a date string into a millisecond timestamp.
/// Supports ISO 8601 and common formats:
///   "2024-01-15"
///   "2024-01-15T12:30:45"
///   "2024-01-15T12:30:45Z"
///   "2024-01-15T12:30:45.123Z"
///   "2024-01-15 12:30:45" (MySQL format)
///   "Jan 15, 2024"
///   Numeric strings (treated as timestamps)
fn parse_date_string(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return f64::NAN;
    }

    // Try as numeric timestamp first
    if let Ok(n) = s.parse::<f64>() {
        return n;
    }

    // Try ISO 8601 / MySQL datetime formats
    // "YYYY-MM-DD" or "YYYY-MM-DDTHH:MM:SS" or "YYYY-MM-DD HH:MM:SS"
    if s.len() >= 10 && s.as_bytes()[4] == b'-' && s.as_bytes()[7] == b'-' {
        let year: i32 = match s[0..4].parse() { Ok(v) => v, Err(_) => return f64::NAN };
        let month: u32 = match s[5..7].parse() { Ok(v) => v, Err(_) => return f64::NAN };
        let day: u32 = match s[8..10].parse() { Ok(v) => v, Err(_) => return f64::NAN };

        if month < 1 || month > 12 || day < 1 || day > 31 {
            return f64::NAN;
        }

        let mut hour: u32 = 0;
        let mut minute: u32 = 0;
        let mut second: u32 = 0;
        let mut millis: u32 = 0;

        // Parse time part if present (after T or space)
        let rest = &s[10..];
        if rest.len() >= 6 && (rest.starts_with('T') || rest.starts_with(' ')) {
            let time_str = &rest[1..];
            if time_str.len() >= 5 && time_str.as_bytes()[2] == b':' {
                hour = match time_str[0..2].parse() { Ok(v) => v, Err(_) => return f64::NAN };
                minute = match time_str[3..5].parse() { Ok(v) => v, Err(_) => return f64::NAN };
                if time_str.len() >= 8 && time_str.as_bytes()[5] == b':' {
                    second = match time_str[6..8].parse() { Ok(v) => v, Err(_) => return f64::NAN };
                    // Milliseconds after '.'
                    if time_str.len() >= 10 && time_str.as_bytes()[8] == b'.' {
                        let ms_end = time_str[9..].find(|c: char| !c.is_ascii_digit()).unwrap_or(time_str.len() - 9);
                        let ms_str = &time_str[9..9 + ms_end];
                        millis = match ms_str.parse::<u32>() {
                            Ok(v) => {
                                // Normalize to 3 digits
                                match ms_str.len() {
                                    1 => v * 100,
                                    2 => v * 10,
                                    3 => v,
                                    _ => v / 10u32.pow(ms_str.len() as u32 - 3),
                                }
                            }
                            Err(_) => 0,
                        };
                    }
                }
            }
        }

        // Convert to timestamp using the same algorithm as timestamp_to_components (inverse)
        let ts = components_to_timestamp(year, month, day, hour, minute, second);
        return (ts * 1000 + millis as i64) as f64;
    }

    f64::NAN
}

/// Convert date components (UTC) to Unix timestamp in seconds.
/// Inverse of timestamp_to_components.
fn components_to_timestamp(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> i64 {
    // Howard Hinnant's civil_from_days (inverse of days_from_civil)
    let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
    let m = if month <= 2 { month as i64 + 9 } else { month as i64 - 3 };
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;

    days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64
}

/// Get timestamp from Date (date.getTime())
/// Since we store dates as timestamps, this is an identity function
#[no_mangle]
pub extern "C" fn js_date_get_time(timestamp: f64) -> f64 {
    timestamp
}

/// Convert Date to ISO 8601 string (date.toISOString())
/// Returns a pointer to a StringHeader
#[no_mangle]
pub extern "C" fn js_date_to_iso_string(timestamp: f64) -> *mut crate::StringHeader {
    use std::alloc::{alloc, Layout};

    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let millis = (ts_ms % 1000).abs() as u32;

    // Calculate date components from Unix timestamp
    // This is a simplified implementation - proper implementation would use chrono crate
    let (year, month, day, hour, minute, second) = timestamp_to_components(secs);

    // Format as ISO 8601: YYYY-MM-DDTHH:mm:ss.sssZ
    let iso_string = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        year, month, day, hour, minute, second, millis
    );

    let bytes = iso_string.as_bytes();
    let len = bytes.len();

    unsafe {
        let layout = Layout::from_size_align(
            std::mem::size_of::<crate::StringHeader>() + len,
            std::mem::align_of::<crate::StringHeader>()
        ).unwrap();

        let ptr = alloc(layout) as *mut crate::StringHeader;
        (*ptr).length = len as u32;

        let data_ptr = (ptr as *mut u8).add(std::mem::size_of::<crate::StringHeader>());
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), data_ptr, len);

        ptr
    }
}

/// Get the full year (date.getFullYear())
#[no_mangle]
pub extern "C" fn js_date_get_full_year(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (year, _, _, _, _, _) = timestamp_to_components(secs);
    year as f64
}

/// Get the month (0-11) (date.getMonth())
#[no_mangle]
pub extern "C" fn js_date_get_month(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (_, month, _, _, _, _) = timestamp_to_components(secs);
    (month - 1) as f64  // JavaScript months are 0-indexed
}

/// Get the day of month (1-31) (date.getDate())
#[no_mangle]
pub extern "C" fn js_date_get_date(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (_, _, day, _, _, _) = timestamp_to_components(secs);
    day as f64
}

/// Get the hour (0-23) (date.getHours())
#[no_mangle]
pub extern "C" fn js_date_get_hours(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (_, _, _, hour, _, _) = timestamp_to_components(secs);
    hour as f64
}

/// Get the minutes (0-59) (date.getMinutes())
#[no_mangle]
pub extern "C" fn js_date_get_minutes(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (_, _, _, _, minute, _) = timestamp_to_components(secs);
    minute as f64
}

/// Get the seconds (0-59) (date.getSeconds())
#[no_mangle]
pub extern "C" fn js_date_get_seconds(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    let secs = ts_ms / 1000;
    let (_, _, _, _, _, second) = timestamp_to_components(secs);
    second as f64
}

/// Get the milliseconds (0-999) (date.getMilliseconds())
#[no_mangle]
pub extern "C" fn js_date_get_milliseconds(timestamp: f64) -> f64 {
    let ts_ms = timestamp as i64;
    (ts_ms % 1000).abs() as f64
}

/// Convert Unix timestamp (seconds) to date components (year, month, day, hour, minute, second)
/// Returns components in UTC
pub fn timestamp_to_components(secs: i64) -> (i32, u32, u32, u32, u32, u32) {
    // Handle negative timestamps (dates before 1970)
    let is_negative = secs < 0;
    let abs_secs = if is_negative { -secs } else { secs } as u64;

    // Extract time of day
    let second = (abs_secs % 60) as u32;
    let minute = ((abs_secs / 60) % 60) as u32;
    let hour = ((abs_secs / 3600) % 24) as u32;

    // Calculate days from Unix epoch
    let mut days = if is_negative {
        -((abs_secs / 86400) as i64) - if abs_secs % 86400 != 0 { 1 } else { 0 }
    } else {
        (abs_secs / 86400) as i64
    };

    // For negative timestamps, adjust time components
    let (hour, minute, second) = if is_negative && abs_secs % 86400 != 0 {
        let remaining = abs_secs % 86400;
        let adjusted = 86400 - remaining;
        (
            ((adjusted / 3600) % 24) as u32,
            ((adjusted / 60) % 60) as u32,
            (adjusted % 60) as u32,
        )
    } else {
        (hour, minute, second)
    };

    // Days since 1970-01-01
    // Using a simplified algorithm based on Howard Hinnant's date algorithms
    let z = days + 719468; // Days from 0000-03-01 to 1970-01-01 is 719468

    let era = if z >= 0 { z / 146097 } else { (z - 146096) / 146097 };
    let doe = (z - era * 146097) as u32; // Day of era [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // Year of era [0, 399]
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // Day of year [0, 365]
    let mp = (5 * doy + 2) / 153; // Month proxy [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // Day [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // Month [1, 12]
    let y = if m <= 2 { y + 1 } else { y };

    (y as i32, m, d, hour, minute, second)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_date_now() {
        let now = js_date_now();
        // Should be a reasonable timestamp (after 2020)
        assert!(now > 1577836800000.0); // 2020-01-01
    }

    #[test]
    fn test_timestamp_to_components() {
        // Test Unix epoch (1970-01-01 00:00:00 UTC)
        let (y, m, d, h, min, s) = timestamp_to_components(0);
        assert_eq!((y, m, d, h, min, s), (1970, 1, 1, 0, 0, 0));

        // Test 2024-01-15 12:30:45 UTC (timestamp: 1705321845)
        let (y, m, d, h, min, s) = timestamp_to_components(1705321845);
        assert_eq!((y, m, d, h, min, s), (2024, 1, 15, 12, 30, 45));
    }
}
