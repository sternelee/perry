# Other System APIs

Additional platform-level APIs.

## Open URL

Open a URL in the default browser or application:

```typescript,no-test
import { openURL } from "perry/system";

openURL("https://example.com");
openURL("mailto:user@example.com");
```

| Platform | Implementation |
|----------|---------------|
| macOS | NSWorkspace.open |
| iOS | UIApplication.open |
| Android | Intent.ACTION_VIEW |
| Windows | ShellExecuteW |
| Linux | xdg-open |
| Web | window.open |

## Dark Mode Detection

```typescript,no-test
import { isDarkMode } from "perry/system";

if (isDarkMode()) {
  // Use dark theme colors
}
```

| Platform | Detection |
|----------|-----------|
| macOS | NSApp.effectiveAppearance |
| iOS | UITraitCollection |
| Android | Configuration.uiMode |
| Windows | Registry (AppsUseLightTheme) |
| Linux | GTK settings |
| Web | prefers-color-scheme media query |

## Clipboard

```typescript,no-test
import { clipboardGet, clipboardSet } from "perry/system";

clipboardSet("Copied text!");
const text = clipboardGet();
```

## Locale Detection

Get the device's language as a 2-letter ISO 639-1 code:

```typescript,no-test
import { getLocale } from "perry/system";

const lang = getLocale(); // "de", "en", "fr", "es", etc.

if (lang === "de") {
  // Use German translations
}
```

| Platform | Implementation |
|----------|---------------|
| macOS | `[NSLocale preferredLanguages]` |
| iOS | `[NSLocale preferredLanguages]` |
| Android | `Locale.getDefault().getLanguage()` |
| Windows | `LANG` / `LC_ALL` environment variable |
| Linux | `LANG` / `LC_ALL` environment variable |
| tvOS | `[NSLocale preferredLanguages]` |
| watchOS | Stub (`"en"`) |

## App Icon Extraction

Get the icon for an application or file as a native Image widget. Useful for building app launchers, file browsers, and search UIs:

```typescript,no-test
import { getAppIcon } from "perry/system";
import { VStack, HStack, Text, Image } from "perry/ui";

// macOS: pass .app bundle path
const finderIcon = getAppIcon("/System/Applications/Finder.app");
const safariIcon = getAppIcon("/Applications/Safari.app");

// Linux: pass .desktop file path
const firefoxIcon = getAppIcon("/usr/share/applications/firefox.desktop");

// Use icons in your UI
HStack(8, [
  finderIcon,
  Text("Finder"),
]);
```

Returns an Image widget handle (32x32 by default). Returns `0` if the icon cannot be loaded.

| Platform | Implementation |
|----------|---------------|
| macOS | `NSWorkspace.shared.icon(forFile:)` — works for any file path, .app bundle, or folder |
| Linux | Parses `.desktop` files for `Icon=` field, looks up via GTK icon theme, falls back to direct image file loading |
| Windows | Not yet implemented (returns 0) |

## Next Steps

- [Overview](overview.md) — All system APIs
- [UI Overview](../ui/overview.md) — Building UIs
