# UI Overview

Perry's `perry/ui` module lets you build native desktop and mobile apps with declarative TypeScript. Your UI code compiles directly to platform-native widgets — AppKit on macOS, UIKit on iOS, GTK4 on Linux, Win32 on Windows, and DOM elements on the web.

## Quick Start

```typescript
import { App, Text, VStack } from "perry/ui";

App({
  title: "My App",
  width: 400,
  height: 300,
  body: VStack(16, [
    Text("Hello from Perry!"),
  ]),
});
```

```bash
perry app.ts -o app && ./app
```

## App Lifecycle

Every Perry UI app starts with `App()`:

```typescript
import { App, VStack, Text } from "perry/ui";

App({
  title: "Window Title",
  width: 800,
  height: 600,
  body: VStack(16, [
    Text("Content here"),
  ]),
});
```

`App({})` accepts a config object with the following properties:

| Property | Type | Description |
|----------|------|-------------|
| `title` | string | Window title |
| `width` | number | Initial window width |
| `height` | number | Initial window height |
| `body` | widget | Root widget |
| `icon` | string | App icon file path (optional) |
| `frameless` | boolean | Remove title bar (optional) |
| `level` | string | Window z-order: `"floating"`, `"statusBar"`, `"modal"` (optional) |
| `transparent` | boolean | Transparent background (optional) |
| `vibrancy` | string | Native blur material, e.g. `"sidebar"` (optional) |
| `activationPolicy` | string | `"regular"`, `"accessory"` (no dock icon), `"background"` (optional) |

See [Multi-Window](multi-window.md) for full documentation on window properties.

### Lifecycle Hooks

```typescript
import { App, onActivate, onTerminate } from "perry/ui";

onActivate(() => {
  console.log("App became active");
});

onTerminate(() => {
  console.log("App is closing");
});

App({ title: "My App", width: 800, height: 600, body: /* ... */ });
```

## Widget Tree

Perry UIs are built as a tree of widgets:

```typescript
import { App, Text, Button, VStack, HStack } from "perry/ui";

App({
  title: "Layout Demo",
  width: 400,
  height: 300,
  body: VStack(16, [
    Text("Header"),
    HStack(8, [
      Button("Left", () => console.log("left")),
      Button("Right", () => console.log("right")),
    ]),
    Text("Footer"),
  ]),
});
```

Widgets are created by calling their constructor functions. Layout containers (`VStack`, `HStack`, `ZStack`) accept a spacing value (in points) followed by an array of child widgets.

## Handle-Based Architecture

Under the hood, each widget is a handle — a small integer that references a native platform object. When you call `Text("hello")`, Perry creates a native `NSTextField` (macOS), `UILabel` (iOS), `GtkLabel` (Linux), or `<span>` (web) and returns a handle you can use to modify it.

```typescript
const label = Text("Hello");
label.setFontSize(18);        // Modifies the native widget
label.setColor("#FF0000");     // Through the handle
```

## Imports

All UI functions are imported from `perry/ui`:

```typescript
import {
  // App lifecycle
  App, onActivate, onTerminate,

  // Widgets
  Text, Button, TextField, SecureField, Toggle, Slider,
  Image, ProgressView, Picker,

  // Layout
  VStack, HStack, ZStack, ScrollView, Spacer, Divider,
  NavigationStack, LazyVStack, Form, Section,

  // State
  State, ForEach,

  // Dialogs
  openFileDialog, saveFileDialog, alert, Sheet,

  // Menus
  menuBarCreate, menuBarAddMenu, contextMenu,

  // Canvas
  Canvas,

  // Table
  Table,

  // Window
  Window,

  // Camera (iOS)
  CameraView, cameraStart, cameraStop, cameraFreeze, cameraUnfreeze,
  cameraSampleColor, cameraSetOnTap,
} from "perry/ui";
```

## Platform Differences

The same code runs on all platforms, but the look and feel matches each platform's native style:

| Feature | macOS | iOS | Linux | Windows | Web |
|---------|-------|-----|-------|---------|-----|
| Buttons | NSButton | UIButton | GtkButton | HWND Button | `<button>` |
| Text | NSTextField | UILabel | GtkLabel | Static HWND | `<span>` |
| Layout | NSStackView | UIStackView | GtkBox | Manual layout | Flexbox |
| Menus | NSMenu | — | GMenu | HMENU | DOM |

Platform-specific behavior is noted on each widget's documentation page.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Arranging widgets
- [State Management](state.md) — Reactive state and bindings
- [Styling](styling.md) — Colors, fonts, sizing
- [Events](events.md) — Click, hover, keyboard
