# Provider Function and Data Fetching

The `provider` function is the heart of a dynamic widget. It fetches data, transforms it, and returns timeline entries that the system renders on schedule.

## Provider Lifecycle

1. The system calls your provider when the widget is first added, when a snapshot is needed, and when the reload policy expires.
2. Your provider runs as native LLVM-compiled code linked into the widget extension.
3. The provider returns one or more timeline entries. The system renders each entry at its scheduled time.
4. After the last entry, the reload policy determines when the provider runs again.

## Basic Provider

```typescript,no-test
import { Widget, Text, VStack } from "perry/widget";

Widget({
  kind: "WeatherWidget",
  displayName: "Weather",
  description: "Current conditions",
  supportedFamilies: ["systemSmall"],

  entryFields: {
    temperature: "number",
    condition: "string",
  },

  provider: async () => {
    const res = await fetch("https://api.weather.example.com/current");
    const data = await res.json();
    return {
      entries: [
        { temperature: data.temp, condition: data.description },
      ],
      reloadPolicy: { after: { minutes: 15 } },
    };
  },

  render: (entry) =>
    VStack([
      Text(`${entry.temperature}°`, { font: "title" }),
      Text(entry.condition, { font: "caption" }),
    ]),
});
```

## Authenticated Requests with Shared Storage

Widgets run in a separate process and cannot access your app's memory. Use `sharedStorage()` to read values that your app has written to a shared container.

### iOS / watchOS: App Groups

On Apple platforms, shared storage maps to `UserDefaults(suiteName:)` backed by an App Group container. Set the `appGroup` field in your widget declaration:

```typescript,no-test
Widget({
  kind: "DashboardWidget",
  displayName: "Dashboard",
  description: "Account summary",
  appGroup: "group.com.example.shared",

  entryFields: {
    revenue: "number",
    users: "number",
  },

  provider: async () => {
    const token = sharedStorage("auth_token");
    const res = await fetch("https://api.example.com/dashboard", {
      headers: { Authorization: `Bearer ${token}` },
    });
    const data = await res.json();
    return {
      entries: [{ revenue: data.revenue, users: data.activeUsers }],
      reloadPolicy: { after: { minutes: 30 } },
    };
  },

  render: (entry) =>
    VStack([
      Text(`$${entry.revenue}`, { font: "title" }),
      Text(`${entry.users} active users`, { font: "caption" }),
    ]),
});
```

Your main app writes the token to the shared container:

```typescript,no-test
import { preferencesSet } from "perry/system";
// In your app's login flow:
preferencesSet("auth_token", token);
```

**Setup requirement (iOS):** Add an App Group capability in Xcode to both the main app target and the widget extension target. The identifier must match the `appGroup` value.

### Android / Wear OS: SharedPreferences

On Android, shared storage maps to `SharedPreferences` with the name `perry_shared`. The generated `Bridge.kt` reads values via `context.getSharedPreferences("perry_shared", MODE_PRIVATE)`.

## Reload Policies

The `reloadPolicy` field controls when the system next calls your provider:

```typescript,no-test
return {
  entries: [{ ... }],
  reloadPolicy: { after: { minutes: 30 } },
};
```

| Policy | Behavior |
|--------|----------|
| `{ after: { minutes: N } }` | Re-fetch after N minutes. Compiles to `.after(Date().addingTimeInterval(N*60))` on iOS and `setFreshnessIntervalMillis(N*60000)` on Wear OS. |
| *(omitted)* | Defaults to 30 minutes on iOS, 30 minutes on Android/Wear OS. |

**Budget limits:** iOS restricts widget refreshes. Typical budget is 40--70 refreshes per day. watchOS is stricter (see [watchOS Complications](watchos.md)). Request only what you need.

## JSON Response Handling

The provider function receives the parsed JSON directly. Entry field types must match your `entryFields` declaration:

```typescript,no-test
entryFields: {
  items: { type: "array", items: { type: "object", fields: { name: "string", count: "number" } } },
  total: "number",
},

provider: async () => {
  const res = await fetch("https://api.example.com/items");
  const data = await res.json();
  return {
    entries: [{
      items: data.results.map((r: any) => ({ name: r.name, count: r.count })),
      total: data.total,
    }],
  };
},
```

## Error Handling

If the fetch fails or JSON parsing throws, the widget extension falls back to the placeholder data:

```typescript,no-test
Widget({
  // ...
  placeholder: { temperature: 0, condition: "Loading..." },

  provider: async () => {
    const res = await fetch("https://api.example.com/weather");
    if (!res.ok) {
      // Return stale/fallback data with a short retry interval
      return {
        entries: [{ temperature: 0, condition: "Unavailable" }],
        reloadPolicy: { after: { minutes: 5 } },
      };
    }
    const data = await res.json();
    return {
      entries: [{ temperature: data.temp, condition: data.desc }],
      reloadPolicy: { after: { minutes: 15 } },
    };
  },
});
```

The `placeholder` field provides data shown in the widget gallery and during loading. If the provider throws an unhandled exception, the generated Swift/Kotlin code catches it and renders the placeholder instead.

## Multiple Timeline Entries

Return multiple entries to schedule future content without re-fetching:

```typescript,no-test
provider: async () => {
  const res = await fetch("https://api.example.com/hourly");
  const hours = await res.json();
  return {
    entries: hours.map((h: any) => ({
      temperature: h.temp,
      condition: h.condition,
    })),
    reloadPolicy: { after: { minutes: 60 } },
  };
},
```

Each entry is rendered at the corresponding date in the timeline. The system transitions between entries automatically.

## Next Steps

- [Configuration](configuration.md) -- User-configurable parameters
- [Cross-Platform Reference](platforms.md) -- Build targets and platform differences
