# Wear OS Tiles

Perry widgets can compile to Wear OS Tiles using `--target wearos-tile`. Tiles are glanceable surfaces in the Wear OS tile carousel and watch face complications.

## Concepts

- **Tiles** are full-screen cards users swipe through on their watch
- **Complications** are small data displays on the watch face
- Perry compiles `Widget({...})` to a `SuspendingTileService` with layout builders

## Supported Components

| Widget API | Wear OS Mapping |
|-----------|----------------|
| `Text` | `LayoutElementBuilders.Text` |
| `VStack` | `LayoutElementBuilders.Column` |
| `HStack` | `LayoutElementBuilders.Row` |
| `Spacer` | `LayoutElementBuilders.Spacer` |
| `Divider` | Spacer with 1dp height |
| `Gauge(circular)` | `LayoutElementBuilders.Arc` + `ArcLine` |
| `Gauge(linear)` | Text fallback |
| `Image` | Resource-based (provide drawable) |

## Example

```typescript,no-test
import { Widget, Text, VStack, Gauge } from "perry/widget";

Widget({
  kind: "StepsTile",
  displayName: "Steps",
  description: "Daily step count",
  supportedFamilies: ["accessoryCircular"],

  provider: async () => {
    return {
      entries: [{ steps: 7500, goal: 10000 }],
      reloadPolicy: { after: { minutes: 60 } }
    }
  },

  render(entry: { steps: number; goal: number }) {
    return VStack([
      Gauge(entry.steps / entry.goal, {
        label: "Steps", style: "circular"
      }),
      Text(`${entry.steps}`, { font: "caption2" }),
    ])
  },
})
```

## Compilation

```bash
perry widget.ts --target wearos-tile --app-bundle-id com.example.app -o tile_out
```

Output:
- `{Name}TileService.kt` — `SuspendingTileService` with tile layout
- `{Name}TileBridge.kt` — JNI bridge for native provider (if provider exists)
- `AndroidManifest_snippet.xml` — Service declaration

## Gradle Integration

Add to your Wear OS module's `build.gradle`:

```groovy
dependencies {
    implementation "com.google.android.horologist:horologist-tiles:0.6.5"
    implementation "androidx.wear.tiles:tiles-material:1.4.0"
    implementation "androidx.wear.tiles:tiles:1.4.0"
}
```

Merge the manifest snippet into your `AndroidManifest.xml`:

```xml
<service
    android:name=".StepsTileService"
    android:exported="true"
    android:permission="com.google.android.wearable.permission.BIND_TILE_PROVIDER">
    <intent-filter>
        <action android:name="androidx.wear.tiles.action.BIND_TILE_PROVIDER" />
    </intent-filter>
</service>
```

## Native Provider

Same as Android phone widgets — Wear OS is Android:
- Target triple: `aarch64-linux-android`
- `libwidget_provider.so` loaded via `System.loadLibrary`
- JNI bridge pattern identical to phone Glance widgets
- `sharedStorage()` uses `SharedPreferences`

## Refresh

Wear Tiles use `freshnessIntervalMillis` on the `Tile` builder. Set via `reloadPolicy: { after: { minutes: N } }` in the provider return value. Default: 60 minutes.
