# Project Configuration

Perry projects use `perry.toml` and `package.json` for configuration. No special config file is required for basic usage, but larger projects benefit from Perry-specific settings.

> **Looking for the full perry.toml reference?** See [perry.toml Reference](../cli/perry-toml.md) for every field, section, platform option, and environment variable.

## Basic Setup

```bash
perry init my-project
cd my-project
```

This creates a `package.json` and a starter `src/index.ts`.

## package.json

```json
{
  "name": "my-project",
  "version": "1.0.0",
  "main": "src/index.ts",
  "perry": {
    "compilePackages": []
  }
}
```

### Perry Configuration

The `perry` field in `package.json` controls compiler behavior:

#### `compilePackages`

List npm packages to compile natively instead of routing through the JavaScript runtime:

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

When a package is listed here, Perry:
1. Resolves the package in `node_modules/`
2. Prefers TypeScript source (`src/index.ts`) over compiled JavaScript (`lib/index.js`)
3. Compiles all functions natively through LLVM
4. Deduplicates across nested `node_modules/` to prevent duplicate linker symbols

This is useful for pure TypeScript/JavaScript packages that don't rely on Node.js APIs. Packages that use native bindings, `eval()`, or dynamic `require()` won't work.

#### `splash`

Configure a native splash screen for iOS and Android. The splash screen appears instantly during cold start, before your app code runs.

**Minimal (both platforms share the same splash):**

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

**Per-platform overrides:**

```json
{
  "perry": {
    "splash": {
      "image": "logo/icon-256.png",
      "background": "#FFF5EE",
      "ios": {
        "image": "logo/splash-ios.png",
        "background": "#FFFFFF"
      },
      "android": {
        "image": "logo/splash-android.png",
        "background": "#FFFFFF"
      }
    }
  }
}
```

**Full custom override (complete control):**

```json
{
  "perry": {
    "splash": {
      "ios": {
        "storyboard": "splash/LaunchScreen.storyboard"
      },
      "android": {
        "layout": "splash/splash_background.xml",
        "theme": "splash/themes.xml"
      }
    }
  }
}
```

| Field | Description |
|-------|-------------|
| `splash.image` | Path to a PNG image, centered on the splash screen (both platforms) |
| `splash.background` | Hex color for the background (default: `#FFFFFF`) |
| `splash.ios.image` | iOS-specific image override |
| `splash.ios.background` | iOS-specific background color |
| `splash.ios.storyboard` | Custom LaunchScreen.storyboard (compiled with ibtool) |
| `splash.android.image` | Android-specific image override |
| `splash.android.background` | Android-specific background color |
| `splash.android.layout` | Custom drawable XML for `windowBackground` |
| `splash.android.theme` | Custom themes.xml |

**Resolution order** per platform:
1. Custom file override (storyboard / layout+theme)
2. Platform-specific image/color (`splash.{platform}.image`)
3. Universal image/color (`splash.image`)
4. No `splash` key → blank white screen (backward compatible)

## Using npm Packages

Perry natively supports many popular npm packages without any configuration:

```typescript
import fastify from "fastify";
import mysql from "mysql2/promise";
import Redis from "ioredis";
import bcrypt from "bcrypt";
```

These are compiled to native code using Perry's built-in implementations. See [Standard Library](../stdlib/overview.md) for the full list.

For packages not natively supported, use `compilePackages` for pure TS/JS packages, or the JavaScript runtime fallback for complex packages.

## Project Structure

Perry is flexible about project structure. Common patterns:

```
my-project/
├── package.json
├── src/
│   └── index.ts
└── node_modules/      # Only needed for compilePackages
```

For UI apps:

```
my-app/
├── package.json
├── src/
│   ├── index.ts       # Main app entry
│   └── components/    # UI components
└── assets/            # Images, etc.
```

## Compilation

```bash
# Compile a file
perry src/index.ts -o build/app

# Compile with a specific target
perry src/index.ts -o build/app --target ios-simulator

# Debug: print intermediate representation
perry src/index.ts --print-hir
```

See [CLI Commands](../cli/commands.md) for all options.

## Next Steps

- [CLI Commands](../cli/commands.md) — All compiler commands and flags
- [Supported Features](../language/supported-features.md) — What TypeScript features work
- [Standard Library](../stdlib/overview.md) — Supported npm packages
