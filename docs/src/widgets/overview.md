# Widgets (WidgetKit) Overview

Perry can compile TypeScript widget declarations to native widget extensions across 4 platforms: iOS (WidgetKit), Android (App Widgets), watchOS (Complications), and Wear OS (Tiles).

## What Are Widgets?

Home screen widgets display glanceable information outside your app. Perry's `perry/widget` module lets you define widgets in TypeScript that compile to each platform's native widget system.

```typescript,no-test
import { Widget, Text, VStack } from "perry/widget";

Widget({
  kind: "MyWidget",
  displayName: "My Widget",
  description: "Shows a greeting",
  entryFields: { name: "string" },
  render: (entry) =>
    VStack([
      Text(`Hello, ${entry.name}!`),
    ]),
});
```

## How It Works

```
TypeScript widget declaration
    ↓ Parse & Lower to WidgetDecl HIR
    ↓ Platform-specific codegen
    ↓
iOS/watchOS: SwiftUI WidgetKit extension (Entry, View, TimelineProvider, WidgetBundle, Info.plist)
Android:    AppWidgetProvider + layout XML + AppWidgetProviderInfo
Wear OS:    TileService + layout
```

The compiler generates a complete native widget extension for each platform — no platform-specific language knowledge required.

## Building

```bash
perry widget.ts --target ios-widget              # iOS WidgetKit extension
perry widget.ts --target android-widget           # Android App Widget
perry widget.ts --target watchos-widget            # watchOS Complication
perry widget.ts --target watchos-widget-simulator   # watchOS Simulator
perry widget.ts --target wearos-tile               # Wear OS Tile
```

Each target produces the appropriate native widget extension for that platform.

## Next Steps

- [Creating Widgets](creating-widgets.md) — Widget() API in detail
- [Components & Modifiers](components.md) — Available widget components
- [Configuration](configuration.md) — Widget configuration options
- [Data Fetching](data-fetching.md) — Timeline providers and data loading
- [Cross-Platform Reference](platforms.md) — Platform-specific details
- [watchOS Complications](watchos.md) — watchOS-specific guide
- [Wear OS Tiles](wearos.md) — Wear OS-specific guide
