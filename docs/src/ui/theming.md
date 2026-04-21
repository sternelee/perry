# Theming

The `perry-styling` package provides a design system bridge for Perry UI — design token codegen and ergonomic styling helpers with compile-time platform detection.

## Installation

```bash
npm install perry-styling
```

## Design Token Codegen

Generate typed theme files from a JSON token definition:

```bash
perry-styling generate --tokens tokens.json --out src/theme.ts
```

### Token Format

```json
{
  "colors": {
    "primary": "#007AFF",
    "primary-dark": "#0A84FF",
    "background": "#FFFFFF",
    "background-dark": "#1C1C1E",
    "text": "#000000",
    "text-dark": "#FFFFFF"
  },
  "spacing": {
    "sm": 4,
    "md": 8,
    "lg": 16,
    "xl": 24
  },
  "radius": {
    "sm": 4,
    "md": 8,
    "lg": 16
  },
  "fontSize": {
    "body": 14,
    "heading": 20,
    "caption": 12
  },
  "borderWidth": {
    "thin": 1,
    "medium": 2
  }
}
```

Colors with a `-dark` suffix are used as the dark mode variant. If no dark variant is provided, the light value is used for both modes. Supported color formats: hex (`#RGB`, `#RRGGBB`, `#RRGGBBAA`), `rgb()`/`rgba()`, `hsl()`/`hsla()`, and CSS named colors.

## Generated Types

The codegen produces typed interfaces:

```typescript,no-test
interface PerryColor {
  r: number; g: number; b: number; a: number; // floats in [0, 1]
}

interface PerryTheme {
  light: { [key: string]: PerryColor };
  dark: { [key: string]: PerryColor };
  spacing: { [key: string]: number };
  radius: { [key: string]: number };
  fontSize: { [key: string]: number };
  borderWidth: { [key: string]: number };
}

interface ResolvedTheme {
  colors: { [key: string]: PerryColor };
  spacing: { [key: string]: number };
  radius: { [key: string]: number };
  fontSize: { [key: string]: number };
  borderWidth: { [key: string]: number };
}
```

## Theme Resolution

Resolve a theme at runtime based on the system's dark mode setting:

```typescript,no-test
import { getTheme } from "perry-styling";
import { theme } from "./theme"; // generated file

const resolved = getTheme(theme);
// resolved.colors.primary → the correct light/dark variant
```

`getTheme()` calls `isDarkMode()` from `perry/system` and returns the appropriate palette.

## Styling Helpers

Ergonomic functions for applying styles to widget handles:

```typescript,no-test
import { applyBg, applyRadius, applyTextColor, applyFontSize, applyGradient } from "perry-styling";

const label = Text("Hello");
applyTextColor(label, resolved.colors.text);
applyFontSize(label, resolved.fontSize.heading);

const card = VStack(16, [/* ... */]);
applyBg(card, resolved.colors.background);
applyRadius(card, resolved.radius.md);
applyGradient(card, startColor, endColor, 0); // 0=vertical, 1=horizontal
```

### Available Helpers

| Function | Description |
|----------|-------------|
| `applyBg(widget, color)` | Set background color |
| `applyRadius(widget, radius)` | Set corner radius |
| `applyTextColor(widget, color)` | Set text color |
| `applyFontSize(widget, size)` | Set font size |
| `applyFontBold(widget)` | Set bold font weight |
| `applyFontFamily(widget, family)` | Set font family |
| `applyWidth(widget, width)` | Set width |
| `applyTooltip(widget, text)` | Set tooltip text |
| `applyBorderColor(widget, color)` | Set border color |
| `applyBorderWidth(widget, width)` | Set border width |
| `applyEdgeInsets(widget, t, r, b, l)` | Set edge insets (padding) |
| `applyOpacity(widget, alpha)` | Set opacity |
| `applyGradient(widget, start, end, dir)` | Set gradient (0=vertical, 1=horizontal) |
| `applyButtonBg(btn, color)` | Set button background |
| `applyButtonTextColor(btn, color)` | Set button text color |
| `applyButtonBordered(btn)` | Set bordered button style |

## Platform Constants

`perry-styling` exports compile-time platform constants based on the `__platform__` built-in:

```typescript,no-test
import { isMac, isIOS, isAndroid, isWindows, isLinux, isDesktop, isMobile } from "perry-styling";

if (isMobile) {
  applyFontSize(label, 16);
} else {
  applyFontSize(label, 14);
}
```

These are constant-folded by LLVM at compile time — dead branches are eliminated with zero runtime cost.

## Next Steps

- [Styling](styling.md) — Widget styling basics
- [State Management](state.md) — Reactive bindings
