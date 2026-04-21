# Widget Configuration

Perry widgets support user-configurable parameters. On iOS/watchOS, these compile to AppIntent configurations (the "Edit Widget" sheet). On Android/Wear OS, they compile to a Configuration Activity.

## Defining Config Fields

Add a `config` object to your `Widget()` declaration. Each field specifies a type, allowed values, a default, and a display title.

```typescript,no-test
import { Widget, Text, VStack, HStack, Spacer } from "perry/widget";

Widget({
  kind: "TopSitesWidget",
  displayName: "Top Sites",
  description: "Your top performing sites",
  supportedFamilies: ["systemSmall", "systemMedium"],
  appGroup: "group.com.example.shared",

  config: {
    sortBy: {
      type: "enum",
      values: ["clicks", "impressions", "ctr", "position"],
      default: "clicks",
      title: "Sort By",
    },
    dateRange: {
      type: "enum",
      values: ["7d", "28d", "90d"],
      default: "7d",
      title: "Date Range",
    },
  },

  entryFields: {
    total: "number",
    label: "string",
  },

  provider: async (config: { sortBy: string; dateRange: string }) => {
    const res = await fetch(
      `https://api.example.com/stats?sort=${config.sortBy}&range=${config.dateRange}`
    );
    const data = await res.json();
    return {
      entries: [{ total: data.total, label: data.label }],
      reloadPolicy: { after: { minutes: 30 } },
    };
  },

  render: (entry) =>
    VStack([
      Text(`${entry.total}`, { font: "title", fontWeight: "bold" }),
      Text(entry.label, { font: "caption", color: "secondary" }),
    ]),
});
```

## Supported Parameter Types

| Type | TypeScript | Description |
|------|-----------|-------------|
| Enum | `{ type: "enum", values: [...], default: "...", title: "..." }` | Picker with fixed choices |
| Boolean | `{ type: "bool", default: true, title: "..." }` | Toggle switch |
| String | `{ type: "string", default: "...", title: "..." }` | Free-text input |

## Accessing Config in the Provider

The `provider` function receives the current config values as its argument. The config object keys match the field names you defined:

```typescript,no-test
provider: async (config: { sortBy: string; dateRange: string }) => {
  // config.sortBy === "clicks" | "impressions" | "ctr" | "position"
  // config.dateRange === "7d" | "28d" | "90d"
  const url = `https://api.example.com/data?sort=${config.sortBy}`;
  const res = await fetch(url);
  const data = await res.json();
  return { entries: [data] };
},
```

When the user changes a config value, the system calls your provider again with the updated config.

## Boolean Config Example

```typescript,no-test
config: {
  showDetails: {
    type: "bool",
    default: true,
    title: "Show Details",
  },
},
```

## Platform Mapping

### iOS / watchOS (AppIntent)

Perry generates a Swift `WidgetConfigurationIntent` struct with `@Parameter` properties and `AppEnum` types for each enum field. The widget uses `AppIntentConfiguration` instead of `StaticConfiguration`.

Generated output (auto-generated, not hand-written):
- `{Name}Intent.swift` -- contains the AppEnum cases and the intent struct
- The provider conforms to `AppIntentTimelineProvider` instead of `TimelineProvider`
- Config values are serialized to JSON and passed to the native provider function

Users configure the widget by long-pressing and selecting "Edit Widget", which presents the system-generated AppIntent UI.

### Android / Wear OS (Configuration Activity)

Perry generates a `{Name}ConfigActivity.kt` with Spinner controls for enum fields and Switch controls for boolean fields. Values are persisted in SharedPreferences keyed by widget ID.

Generated output:
- `{Name}ConfigActivity.kt` -- Activity with UI controls and a Save button
- `widget_info_{name}.xml` -- includes `android:configure` pointing to the config activity
- AndroidManifest snippet includes an `<activity>` entry with `APPWIDGET_CONFIGURE` intent filter

The config activity launches automatically when the user first adds the widget.

## Build Commands

```bash
# iOS
perry widget.ts --target ios-widget --app-bundle-id com.example.app -o widget_out

# Android
perry widget.ts --target android-widget --app-bundle-id com.example.app -o widget_out
```

## Next Steps

- [Data Fetching](data-fetching.md) -- Provider function and shared storage
- [Components](components.md) -- Available widget components
- [Cross-Platform Reference](platforms.md) -- Feature matrix and build targets
