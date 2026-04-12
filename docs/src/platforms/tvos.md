# tvOS

Perry can compile TypeScript apps for Apple TV devices and the tvOS Simulator.

tvOS uses UIKit (the same framework as iOS), so Perry's tvOS support shares the same UIKit-based widget system. The primary difference is input: Apple TV apps are controlled via the Siri Remote and game controllers rather than touch, and all apps run full-screen.

## Requirements

- macOS host (cross-compilation from Linux/Windows is not supported)
- Xcode (full install) for tvOS SDK and Simulator
- Rust tvOS targets:
  ```bash
  rustup target add aarch64-apple-tvos aarch64-apple-tvos-sim
  ```

## Building for Simulator

```bash
perry compile app.ts -o app --target tvos-simulator
```

This produces an ARM64 binary linked with `clang` against the tvOS Simulator SDK, wrapped in a `.app` bundle.

## Building for Device

```bash
perry compile app.ts -o app --target tvos
```

This produces an ARM64 binary for physical Apple TV hardware.

## Running with `perry run`

```bash
perry run tvos                        # Auto-detect booted Apple TV simulator
perry run tvos --simulator <UDID>     # Target a specific simulator
```

Perry auto-discovers booted Apple TV simulators. To install and launch manually:

```bash
xcrun simctl install booted app.app
xcrun simctl launch booted com.perry.app
```

## UI Toolkit

Perry maps UI widgets to UIKit controls on tvOS, identical to iOS:

| Perry Widget | UIKit Class | Notes |
|-------------|------------|-------|
| Text | UILabel | |
| Button | UIButton | Focus-based navigation |
| TextField | UITextField | On-screen keyboard via Siri Remote |
| Toggle | UISwitch | |
| Slider | UISlider | |
| Picker | UIPickerView | |
| Image | UIImageView | |
| VStack/HStack | UIStackView | |
| ScrollView | UIScrollView | Focus-based scrolling |

### Focus Engine

tvOS uses a **focus-based navigation model** instead of direct touch. The Siri Remote's touchpad and directional buttons move focus between focusable views. Perry widgets that support interaction (buttons, text fields, toggles, etc.) are automatically focusable.

## Game Engine Support

tvOS is particularly well-suited for game engines. When using a native library like [Bloom](https://bloomengine.dev), the game engine handles its own windowing, rendering, and input:

```typescript
import { initWindow, windowShouldClose, beginDrawing, endDrawing,
         clearBackground, isGamepadButtonDown, Colors } from "bloom";

initWindow(1920, 1080, "My Apple TV Game");

while (!windowShouldClose()) {
  beginDrawing();
  clearBackground(Colors.BLACK);

  if (isGamepadButtonDown(0)) {
    // A button (Siri Remote select) pressed
  }

  endDrawing();
}
```

### Input on tvOS

The Siri Remote acts as a game controller:

| Input | Mapping |
|-------|---------|
| Touchpad swipe | Gamepad axes 0/1 (left stick) |
| Touchpad click (Select) | Gamepad button 0 (A) + mouse button 0 |
| Menu button | Gamepad button 1 (B) |
| Play/Pause button | Gamepad button 9 (Start) |
| Arrow presses (up/down/left/right) | Gamepad D-pad buttons (12-15) |

Extended game controllers (MFi, PlayStation, Xbox) are fully supported with all axes, buttons, triggers, and D-pad mapped through the standard gamepad API.

## App Lifecycle

tvOS apps use `UIApplicationMain` with the same lifecycle as iOS. When using `perry/ui`:

```typescript
import { App, Text, VStack } from "perry/ui";

App({
  title: "My TV App",
  width: 1920,
  height: 1080,
  body: VStack(16, [
    Text("Hello, Apple TV!"),
  ]),
});
```

When using a game engine with `--features ios-game-loop`, the runtime starts `UIApplicationMain` on the main thread and runs your game code on a dedicated game thread.

## Configuration

Configure tvOS settings in `perry.toml`:

```toml
[tvos]
bundle_id = "com.example.mytvapp"
deployment_target = "17.0"
```

## Platform Detection

Use `__platform__ === 6` to detect tvOS at compile time:

```typescript
declare const __platform__: number;

if (__platform__ === 6) {
  console.log("Running on tvOS");
}
```

## App Bundle

Perry generates a `.app` bundle with an `Info.plist` containing:

| Key | Value | Notes |
|-----|-------|-------|
| `UIDeviceFamily` | `[3]` | Apple TV |
| `MinimumOSVersion` | `17.0` | tvOS 17+ |
| `UIRequiresFullScreen` | `true` | All tvOS apps are full-screen |
| `UILaunchStoryboardName` | `LaunchScreen` | Required by tvOS |

## Limitations

tvOS has inherent platform constraints compared to other Perry targets:

- **No camera**: Apple TV has no camera hardware
- **No clipboard**: UIPasteboard is not available on tvOS
- **No file dialogs**: No document picker
- **No QR code**: No camera for scanning
- **No multi-window**: Single full-screen window only
- **No direct touch**: Input is via Siri Remote focus engine and game controllers
- **Resolution**: Design for 1920x1080 (1080p) or 3840x2160 (4K) displays

## Differences from iOS

| Aspect | tvOS | iOS |
|--------|------|-----|
| **Input** | Siri Remote + game controllers (focus engine) | Direct touch |
| **Display** | Full-screen only (1080p/4K) | Variable screen sizes |
| **Device family** | `[3]` (Apple TV) | `[1, 2]` (iPhone/iPad) |
| **Camera** | Not available | Available |
| **Clipboard** | Not available | Available |
| **Deployment target** | 17.0 | 17.0 |
| **UI framework** | UIKit (same as iOS) | UIKit |

## Next Steps

- [iOS](ios.md) — iOS platform reference (shared UIKit base)
- [watchOS](watchos.md) — watchOS platform reference
- [Platform Overview](overview.md) — All platforms
