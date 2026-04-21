# watchOS Complications

Perry widgets can compile to watchOS WidgetKit complications using `--target watchos-widget`. The same `Widget({...})` source produces both iOS and watchOS widgets — the supported families determine the rendering.

## Accessory Families

watchOS complications use accessory families instead of system families:

| Family | Size | Best For |
|--------|------|----------|
| `accessoryCircular` | ~76x76pt | Single icon, number, or Gauge |
| `accessoryRectangular` | ~160x76pt | 2-3 lines of text |
| `accessoryInline` | Single line | Short text only |

## Gauge Component

The `Gauge` component is designed for watchOS circular complications:

```typescript,no-test
import { Widget, Text, VStack, Gauge } from "perry/widget";

Widget({
  kind: "QuickStats",
  displayName: "Quick Stats",
  supportedFamilies: ["accessoryCircular", "accessoryRectangular"],

  render(entry: { progress: number; label: string }, family) {
    if (family === "accessoryCircular") {
      return Gauge(entry.progress, {
        label: "Done", style: "circular"
      })
    }
    return VStack([
      Text(entry.label, { font: "headline" }),
      Gauge(entry.progress, { label: "Progress", style: "linear" }),
    ])
  },
})
```

### Gauge Styles

- **`circular`** — Ring gauge, maps to `.gaugeStyle(.accessoryCircularCapacity)` in SwiftUI
- **`linear`** / **`linearCapacity`** — Horizontal bar, maps to `.gaugeStyle(.linearCapacity)`

## Refresh Budgets

watchOS has stricter refresh budgets than iOS:
- Recommended: refresh every 60 minutes (`reloadPolicy: { after: { minutes: 60 } }`)
- Maximum: system may throttle more aggressively than iOS
- Background refresh uses `BackgroundTask` framework

## Compilation

```bash
# For Apple Watch device
perry widget.ts --target watchos-widget --app-bundle-id com.example.app -o widget_out

# For Apple Watch Simulator
perry widget.ts --target watchos-widget-simulator --app-bundle-id com.example.app -o widget_out
```

Build:
```bash
xcrun --sdk watchos swiftc -target arm64-apple-watchos9.0 \
  widget_out/*.swift \
  -framework WidgetKit -framework SwiftUI \
  -o widget_out/WidgetExtension
```

## Configuration

- watchOS 10+ supports AppIntent for widget configuration (same as iOS 17+)
- Older watchOS versions automatically get `StaticConfiguration` fallback
- `config` params work identically to iOS

## Memory Considerations

watchOS widget extensions have tighter memory limits (~15-20MB) compared to iOS (~30MB). The provider-only compilation approach is critical — only the data-fetching code runs natively, keeping memory usage minimal.
