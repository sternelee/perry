# Internationalization (i18n)

Perry's i18n system lets you write natural English strings and have them automatically translated at compile time. Zero ceremony, near-zero runtime cost.

```typescript,no-test
import { Button, Text } from "perry/ui";

Button("Next")                              // Automatically localized
Text("Hello, {name}!", { name: user.name }) // With interpolation
```

## Design Principles

- **Zero ceremony**: String literals in UI components are automatically localizable keys
- **Compile-time validation**: Missing translations, parameter mismatches, and plural form errors caught during build
- **Embedded string table**: All translations baked into the binary as a flat 2D table. Near-zero runtime lookup cost
- **Platform-native locale detection**: Uses OS APIs on every platform (no env vars needed on mobile)

## Quick Start

### 1. Add i18n config to perry.toml

```toml
[i18n]
locales = ["en", "de"]
default_locale = "en"
```

### 2. Extract strings from your code

```bash
perry i18n extract src/main.ts
```

This scans your source files and creates `locales/en.json` and `locales/de.json`:

```json
// locales/en.json
{
  "Next": "Next",
  "Back": "Back"
}

// locales/de.json (empty values = needs translation)
{
  "Next": "",
  "Back": ""
}
```

### 3. Translate

Fill in `locales/de.json`:

```json
{
  "Next": "Weiter",
  "Back": "Zurck"
}
```

### 4. Build

```bash
perry compile src/main.ts -o myapp
```

Perry validates all translations at compile time and bakes them into the binary. At runtime, the app detects the user's system locale and shows the right language.

## How It Works

1. **Detection**: String literals in UI component calls (`Button`, `Text`, `Label`, etc.) are automatically treated as i18n keys
2. **Transform**: The compiler replaces `Expr::String("Next")` with `Expr::I18nString { key: "Next", string_idx: 0 }` in the HIR
3. **Codegen**: For each `I18nString`, the compiler emits a locale branch that selects the correct translation at runtime
4. **Locale detection**: At startup, `perry_i18n_init()` detects the system locale via native APIs and sets the global locale index

## Locale Detection

| Platform | Method |
|----------|--------|
| macOS | `CFLocaleCopyCurrent()` (CoreFoundation) |
| iOS | `CFLocaleCopyCurrent()` (CoreFoundation) |
| Android | `__system_property_get("persist.sys.locale")` |
| Windows | `GetUserDefaultLocaleName()` (Win32) |
| Linux | `LANG` / `LC_ALL` / `LC_MESSAGES` env vars |

The detected locale is fuzzy-matched against your configured locales: `de_DE.UTF-8` matches `de`, `en-US` matches `en`, etc.

## Platform Output

When compiling for mobile targets, Perry generates platform-native locale resources alongside the binary:

| Platform | Output |
|----------|--------|
| iOS/macOS | `{locale}.lproj/Localizable.strings` inside `.app` bundle |
| Android | `res/values-{locale}/strings.xml` |
| Desktop | Strings embedded in binary (no extra files) |

## Next Steps

- [Interpolation & Plurals](interpolation.md)
- [Formatting](formatting.md)
- [CLI Tools](cli.md)
