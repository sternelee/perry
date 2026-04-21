# macOS

macOS is Perry's primary development platform. It uses AppKit for native UI.

## Requirements

- macOS 13+ (Ventura or later)
- Xcode Command Line Tools: `xcode-select --install`

## Building

```bash
# macOS is the default target
perry app.ts -o app
./app
```

No additional flags needed — macOS is the default compilation target.

## UI Toolkit

Perry maps UI widgets to AppKit controls:

| Perry Widget | AppKit Class |
|-------------|-------------|
| Text | NSTextField (label mode) |
| Button | NSButton |
| TextField | NSTextField |
| SecureField | NSSecureTextField |
| Toggle | NSSwitch |
| Slider | NSSlider |
| Picker | NSPopUpButton |
| Image | NSImageView |
| VStack/HStack | NSStackView |
| ScrollView | NSScrollView |
| Table | NSTableView |
| Canvas | NSView + Core Graphics |

## Code Signing

For distribution, apps need to be signed. Perry supports automatic signing:

```bash
perry publish
```

This auto-detects your signing identity from the macOS Keychain, exports it to a temporary `.p12` file, and signs the binary.

For manual signing:

```bash
codesign --sign "Developer ID Application: Your Name" ./app
```

## App Store Distribution

```bash
perry app.ts -o MyApp
# Sign with App Store certificate
codesign --sign "3rd Party Mac Developer Application: Your Name" MyApp
# Package
productbuild --sign "3rd Party Mac Developer Installer: Your Name" --component MyApp /Applications MyApp.pkg
```

## macOS-Specific Features

- **Menu bar**: Full NSMenu support with keyboard shortcuts
- **Toolbar**: NSToolbar integration
- **Dock icon**: Automatic for GUI apps
- **Dark mode**: `isDarkMode()` detects system appearance
- **Keychain**: Secure storage via Security.framework
- **Notifications**: Local notifications via UNUserNotificationCenter
- **File dialogs**: NSOpenPanel/NSSavePanel

## System APIs

```typescript,no-test
import { openURL, isDarkMode, preferencesSet, preferencesGet } from "perry/system";

openURL("https://example.com");          // Opens in default browser
const dark = isDarkMode();                // Check appearance
preferencesSet("key", "value");           // NSUserDefaults
const val = preferencesGet("key");        // NSUserDefaults
```

## Next Steps

- [iOS](ios.md) — Cross-compile for iPhone/iPad
- [UI Overview](../ui/overview.md) — Full UI documentation
- [System APIs](../system/overview.md) — System integration
