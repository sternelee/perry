# Native Extensions

Perry supports native extensions — packages that bundle platform-specific code (Rust, Swift, JNI) alongside a TypeScript API. Unlike [dynamic plugins](overview.md) loaded at runtime, native extensions are compiled directly into your binary.

Native extensions are how you access platform APIs that aren't part of Perry's built-in [System APIs](../system/overview.md) or [Standard Library](../stdlib/overview.md). Examples include [App Store Review](appstore-review.md) and StoreKit for in-app purchases.

## Using a native extension

### 1. Add the extension to your project

Place the extension directory alongside your project, or in a shared extensions directory:

```
my-app/
├── package.json
├── src/
│   └── index.ts
└── extensions/
    └── perry-appstore-review/
        ├── package.json
        ├── src/
        │   └── index.ts
        ├── crate-ios/
        ├── crate-android/
        └── crate-stub/
```

### 2. Compile with `--bundle-extensions`

Pass the extensions directory when building:

```bash
perry src/index.ts -o app --target ios --bundle-extensions ./extensions
```

Perry discovers every subdirectory with a `package.json`, compiles its native crates for the target platform, and links them into your binary.

### 3. Import and use

```typescript,no-test
import { requestReview } from "perry-appstore-review";

await requestReview();
```

The import resolves at compile time to the extension's entry point. No runtime module loading is involved — the function compiles to a direct native call.

## How native extensions work

A native extension is a directory with a `package.json` that declares a `perry.nativeLibrary` section. This tells Perry which native functions exist, their signatures, and which Rust crate to compile for each platform.

### package.json manifest

```json
{
  "name": "perry-appstore-review",
  "version": "0.1.0",
  "main": "src/index.ts",
  "perry": {
    "nativeLibrary": {
      "functions": [
        { "name": "sb_appreview_request", "params": [], "returns": "f64" }
      ],
      "targets": {
        "ios": {
          "crate": "crate-ios",
          "lib": "libperry_appreview.a",
          "frameworks": ["StoreKit"]
        },
        "android": {
          "crate": "crate-android",
          "lib": "libperry_appreview.a",
          "frameworks": []
        },
        "macos": {
          "crate": "crate-ios",
          "lib": "libperry_appreview.a",
          "frameworks": ["StoreKit"]
        }
      }
    }
  }
}
```

#### `functions`

Each entry declares a native function the extension exports:

| Field | Description |
|-------|-------------|
| `name` | Symbol name — must match the `#[no_mangle]` Rust function exactly |
| `params` | Array of LLVM types: `"i64"` for pointers/strings, `"f64"` for numbers, `"i32"` for integers |
| `returns` | Return type — typically `"f64"` (NaN-boxed value or promise handle) |

#### `targets`

Each target platform maps to a Rust crate that implements the native functions:

| Field | Description |
|-------|-------------|
| `crate` | Relative path to the Rust crate directory |
| `lib` | Name of the static library produced by `cargo build` |
| `frameworks` | System frameworks to link (iOS/macOS only) |

Multiple targets can share the same crate (e.g., iOS and macOS often share an implementation). Platforms without an entry fall back to the stub.

### Extension directory layout

```
perry-appstore-review/
├── package.json              # Manifest with perry.nativeLibrary
├── src/
│   └── index.ts              # TypeScript API (what users import)
├── crate-ios/                # iOS/macOS native implementation
│   ├── Cargo.toml            # [lib] crate-type = ["staticlib"]
│   ├── build.rs              # Compiles Swift if needed
│   ├── src/
│   │   └── lib.rs            # Rust FFI: #[no_mangle] pub extern "C" fn ...
│   └── swift/
│       └── bridge.swift      # Swift bridge for Apple APIs (@_cdecl)
├── crate-android/            # Android native implementation
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs            # Rust FFI with JNI calls
└── crate-stub/               # Fallback for unsupported platforms
    ├── Cargo.toml
    └── src/
        └── lib.rs            # Returns error immediately
```

### TypeScript side

The `src/index.ts` declares native functions and optionally wraps them in a friendlier API:

```typescript,no-test
// Declare the native function (name must match package.json)
declare function sb_appreview_request(): number;

// Wrap it with a proper TypeScript signature
export async function requestReview(): Promise<void> {
  await (sb_appreview_request() as any);
}
```

`declare function` tells Perry the function is provided by native code. The raw return type is `number` because all values cross the FFI boundary as NaN-boxed `f64` values. Promise handles are NaN-boxed pointers that Perry's runtime knows how to `await`.

### Rust side

Each platform crate is a `staticlib` that implements the declared functions using `#[no_mangle] pub extern "C"`:

```rust
// Perry runtime FFI
extern "C" {
    fn js_promise_new() -> *mut u8;
    fn js_promise_resolve(promise: *mut u8, value: f64);
    fn js_nanbox_string(ptr: i64) -> f64;
    fn js_nanbox_pointer(ptr: i64) -> f64;
}

#[no_mangle]
pub extern "C" fn sb_appreview_request() -> f64 {
    unsafe {
        let promise = js_promise_new();
        // ... call platform API, resolve promise when done ...
        js_nanbox_pointer(promise as i64)
    }
}
```

Key runtime functions available to native code:

| Function | Purpose |
|----------|---------|
| `js_promise_new()` | Create a new Perry promise, returns pointer |
| `js_promise_resolve(promise, value)` | Resolve a promise with a NaN-boxed value |
| `js_nanbox_string(ptr)` | Convert a C string pointer to a NaN-boxed string |
| `js_nanbox_pointer(ptr)` | Convert a pointer to a NaN-boxed object reference |
| `js_get_string_pointer_unified(val)` | Extract string pointer from a NaN-boxed value |
| `js_string_from_bytes(ptr, len)` | Create a Perry string from bytes |

### Swift bridge (iOS/macOS)

Apple platform APIs are often easiest to call from Swift. The pattern is:

1. Write a Swift file with `@_cdecl("function_name")` exports
2. Compile it to a static library in `build.rs`
3. Call the Swift functions from Rust via `extern "C"`

```swift
import StoreKit

typealias Callback = @convention(c) (UnsafeMutableRawPointer, UnsafePointer<CChar>) -> Void

@_cdecl("swift_appreview_request")
func swiftRequestReview(_ callback: @escaping Callback, _ context: UnsafeMutableRawPointer) {
    DispatchQueue.main.async {
        if let scene = UIApplication.shared.connectedScenes
            .first(where: { $0.activationState == .foregroundActive }) as? UIWindowScene {
            SKStoreReviewController.requestReview(in: scene)
        }
        let result = "{\"success\":true}"
        result.withCString { callback(context, $0) }
    }
}
```

The `build.rs` compiles the Swift source into a static library using `swiftc`, targeting the correct platform SDK:

```rust
// build.rs (simplified)
fn main() {
    // Detect target: aarch64-apple-ios → arm64-apple-ios16.0, iphoneos SDK
    // Compile: swiftc -emit-library -static -target ... -sdk ... -framework StoreKit
    // Link:    cargo:rustc-link-lib=static=review_bridge
}
```

### JNI bridge (Android)

Android platform APIs are accessed through JNI. The pattern:

1. Get the `JavaVM` via `JNI_GetCreatedJavaVMs()`
2. Attach the current thread to get a `JNIEnv`
3. Call Java/Kotlin APIs through JNI method invocations
4. Resolve the Perry promise with the result

```rust
use jni::JavaVM;
use jni::objects::JValue;

fn request_review_impl() -> Result<(), String> {
    let vm = get_java_vm()?;
    let mut env = vm.attach_current_thread_as_daemon().map_err(|e| e.to_string())?;

    // Get Activity from PerryBridge
    let bridge = env.find_class("com/perry/app/PerryBridge").map_err(|e| e.to_string())?;
    let activity = env.call_static_method(bridge, "getActivity", "()Landroid/app/Activity;", &[])
        .map_err(|e| e.to_string())?.l().map_err(|e| e.to_string())?;

    // Call platform APIs via JNI...
    Ok(())
}
```

If the Android implementation requires a Java library (e.g., Google Play In-App Review), the app's `build.gradle` must include the dependency. Document this requirement clearly for your extension's users.

### Stub crate

For platforms without a native implementation, the stub immediately resolves the promise with an error:

```rust
#[no_mangle]
pub extern "C" fn sb_appreview_request() -> f64 {
    unsafe {
        let promise = js_promise_new();
        let msg = "{\"error\":\"Not available on this platform\"}";
        let c_str = std::ffi::CString::new(msg).unwrap();
        let val = js_nanbox_string(c_str.as_ptr() as i64);
        std::mem::forget(c_str);
        js_promise_resolve(promise, val);
        js_nanbox_pointer(promise as i64)
    }
}
```

## Build requirements

| Platform | Requirements |
|----------|-------------|
| iOS | macOS host, Xcode, `rustup target add aarch64-apple-ios` |
| iOS Simulator | macOS host, Xcode, `rustup target add aarch64-apple-ios-sim` |
| macOS | macOS host, Xcode Command Line Tools |
| Android | Android NDK, `rustup target add aarch64-linux-android` |

When Perry encounters a `perry.nativeLibrary` manifest during compilation, it:

1. Selects the crate for the current `--target` platform
2. Runs `cargo build --release --target <triple>` in the crate directory
3. Links the resulting `.a` static library into the final binary
4. Adds any declared frameworks (e.g., `-framework StoreKit`)

## Creating your own native extension

1. Create the directory structure shown above
2. Define your functions in `package.json` under `perry.nativeLibrary`
3. Implement each function in the platform crates with matching `#[no_mangle] pub extern "C"` signatures
4. Write a TypeScript entry point that declares and optionally wraps the native functions
5. Add a stub crate for unsupported platforms
6. Test with `--bundle-extensions`:
   ```bash
   perry app.ts --target ios-simulator --bundle-extensions ./extensions
   ```

## Next Steps

- [App Store Review](appstore-review.md) — Native review prompt extension (iOS/Android)
- [Creating Plugins](creating-plugins.md) — Dynamic plugins loaded at runtime
- [Overview](overview.md) — Plugin system overview
