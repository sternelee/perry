# iOS

Perry can cross-compile TypeScript apps for iOS devices and the iOS Simulator.

## Requirements

- macOS host (cross-compilation from Linux/Windows is not supported)
- Xcode (full install, not just Command Line Tools) for iOS SDK and Simulator
- Rust iOS targets:
  ```bash
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim
  ```

## Building for Simulator

```bash
perry app.ts -o app --target ios-simulator
```

This uses LLVM cross-compilation with the iOS Simulator SDK. The binary can be run in the Xcode Simulator.

## Building for Device

```bash
perry app.ts -o app --target ios
```

This produces an ARM64 binary for physical iOS devices. You'll need to code sign and package it in an `.app` bundle for deployment.

## Running with `perry run`

The easiest way to build and run on iOS is `perry run`:

```bash
perry run ios              # Auto-detect device/simulator
perry run ios --console    # Stream live stdout/stderr
perry run ios --remote     # Use Perry Hub build server
```

Perry auto-discovers available simulators (via `simctl`) and physical devices (via `devicectl`). When multiple targets are found, an interactive prompt lets you choose.

For physical devices, Perry handles code signing automatically — it reads your signing identity and team ID from `~/.perry/config.toml` (set up via `perry setup ios`), embeds the provisioning profile, and signs the `.app` before installing.

If you don't have the iOS cross-compilation toolchain installed locally, `perry run ios` automatically falls back to Perry Hub's remote build server.

## UI Toolkit

Perry maps UI widgets to UIKit controls:

| Perry Widget | UIKit Class |
|-------------|------------|
| Text | UILabel |
| Button | UIButton (TouchUpInside) |
| TextField | UITextField |
| SecureField | UITextField (secureTextEntry) |
| Toggle | UISwitch |
| Slider | UISlider (Float32, cast at boundary) |
| Picker | UIPickerView |
| Image | UIImageView |
| VStack/HStack | UIStackView |
| ScrollView | UIScrollView |

## App Lifecycle

iOS apps use `UIApplicationMain` with a deferred creation pattern:

```typescript,no-test
import { App, Text, VStack } from "perry/ui";

App({
  title: "My iOS App",
  width: 400,
  height: 800,
  body: VStack(16, [
    Text("Hello, iPhone!"),
  ]),
});
```

The `App()` call triggers `UIApplicationMain`, and your render function is called via `PerryAppDelegate` once the app is ready.

## iOS Widgets (WidgetKit)

Perry can compile TypeScript widget declarations to native SwiftUI WidgetKit extensions:

```bash
perry widget.ts --target ios-widget
```

See [Widgets (WidgetKit)](../widgets/overview.md) for details.

## Splash Screen

Perry auto-generates a native `LaunchScreen.storyboard` from the `perry.splash` config in `package.json`. The splash screen appears instantly during cold start.

```json
{
  "perry": {
    "splash": {
      "image": "logo/icon-256.png",
      "background": "#FFF5EE"
    }
  }
}
```

The image is centered at 128x128pt with `scaleAspectFit`. You can provide a custom storyboard for full control:

```json
{
  "perry": {
    "splash": {
      "ios": { "storyboard": "splash/LaunchScreen.storyboard" }
    }
  }
}
```

See [Project Configuration](../getting-started/project-config.md#splash) for the full config reference.

## Resource Bundling

Perry automatically bundles `logo/` and `assets/` directories from your project root into the `.app` bundle. These resources are available at runtime via standard file APIs relative to the app bundle path.

## Keyboard Avoidance

Perry apps automatically handle keyboard avoidance on iOS. When the keyboard appears, the root view adjusts its bottom constraint with an animated layout transition, and focused TextFields are auto-scrolled into view above the keyboard.

## Differences from macOS

- **No menu bar**: iOS doesn't support menu bars. Use toolbar or navigation patterns.
- **Touch events**: `onHover` is not available. Use `onClick` (mapped to touch).
- **Slider precision**: iOS UISlider uses Float32 internally (automatically converted).
- **File dialogs**: Limited to UIDocumentPicker.
- **Keyboard shortcuts**: Not applicable on iOS.

## Next Steps

- [Widgets (WidgetKit)](../widgets/overview.md) — iOS home screen widgets
- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
