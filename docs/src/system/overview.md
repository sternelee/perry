# System APIs Overview

The `perry/system` module provides access to platform-native system features: preferences, secure storage, notifications, URL opening, dark mode detection, and app introspection.

```typescript,no-test
import { openURL, isDarkMode, preferencesSet, preferencesGet, getAppIcon } from "perry/system";
```

## Available APIs

| Function | Description | Platforms |
|----------|------------|-----------|
| `openURL(url)` | Open URL in default browser/app | All |
| `isDarkMode()` | Check system dark mode | All |
| `preferencesSet(key, value)` | Store a preference | All |
| `preferencesGet(key)` | Read a preference | All |
| `keychainSet(key, value)` | Secure storage write | All |
| `keychainGet(key)` | Secure storage read | All |
| `sendNotification(title, body)` | Local notification | All |
| `clipboardGet()` | Read clipboard | All |
| `clipboardSet(text)` | Write clipboard | All |
| `audioStart()` | Start microphone capture | All |
| `audioStop()` | Stop microphone capture | All |
| `audioGetLevel()` | Current dB(A) sound level | All |
| `audioGetPeak()` | Current peak amplitude (0–1) | All |
| `audioGetWaveformSamples(n)` | Recent dB samples for visualization | All |
| `getLocale()` | Device language code (e.g. `"de"`, `"en"`) | All |
| `getDeviceModel()` | Device model identifier | All |
| `getAppIcon(path)` | Get app/file icon as Image widget | macOS, Linux |

## Quick Example

```typescript,no-test
import { isDarkMode, preferencesGet, preferencesSet, openURL } from "perry/system";

// Detect dark mode
if (isDarkMode()) {
  console.log("Dark mode is active");
}

// Store user preferences
preferencesSet("theme", "dark");
const theme = preferencesGet("theme");

// Open a URL
openURL("https://example.com");
```

## Next Steps

- [Preferences](preferences.md)
- [Keychain](keychain.md)
- [Notifications](notifications.md)
- [Audio Capture](audio.md)
- [Other](other.md)
