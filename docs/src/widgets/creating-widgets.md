# Creating Widgets

Define home screen widgets using the `Widget()` function.

## Widget Declaration

```typescript,no-test
import { Widget, Text, VStack, HStack, Image, Spacer } from "perry/widget";

Widget({
  kind: "WeatherWidget",
  displayName: "Weather",
  description: "Shows current weather",
  entryFields: {
    temperature: "number",
    condition: "string",
    location: "string",
  },
  render: (entry) =>
    VStack([
      HStack([
        Text(entry.location),
        Spacer(),
        Image("cloud.sun.fill"),
      ]),
      Text(`${entry.temperature}°`),
      Text(entry.condition),
    ]),
});
```

## Widget Options

| Property | Type | Description |
|----------|------|-------------|
| `kind` | `string` | Unique identifier for the widget |
| `displayName` | `string` | Name shown in widget gallery |
| `description` | `string` | Description in widget gallery |
| `entryFields` | `object` | Data fields with types (`"string"`, `"number"`, `"boolean"`, arrays, optionals, objects) |
| `render` | `function` | Render function receiving entry data, returns widget tree. Optional 2nd param for family. |
| `config` | `object` | Configurable parameters the user can edit (see below) |
| `provider` | `function` | Timeline provider function for dynamic data (see below) |
| `appGroup` | `string` | App group identifier for sharing data with the host app |

## Entry Fields

Entry fields define the data your widget displays. Each field has a name and type:

```typescript,no-test
entryFields: {
  title: "string",
  count: "number",
  isActive: "boolean",
}
```

### Array, Optional, and Object Fields

Entry fields support richer types beyond primitives:

```typescript,no-test
entryFields: {
  items: [{ name: "string", value: "number" }],  // Array of objects
  subtitle: "string?",                             // Optional string
  stats: { wins: "number", losses: "number" },     // Nested object
}
```

These compile to a Swift `TimelineEntry` struct:

```swift
struct WeatherEntry: TimelineEntry {
    let date: Date
    let temperature: Double
    let condition: String
    let location: String
}
```

## Conditionals in Render

Use ternary expressions for conditional rendering:

```typescript,no-test
render: (entry) =>
  VStack([
    Text(entry.isActive ? "Active" : "Inactive"),
    entry.count > 0 ? Text(`${entry.count} items`) : Spacer(),
  ]),
```

## Template Literals

Template literals in widget text are compiled to Swift string interpolation:

```typescript,no-test
Text(`${entry.name}: ${entry.score} points`)
// Compiles to: Text("\(entry.name): \(entry.score) points")
```

## Configuration Parameters

The `config` field defines user-editable parameters that appear in the widget's edit UI:

```typescript,no-test
Widget({
  kind: "CityWeather",
  displayName: "City Weather",
  description: "Weather for a chosen city",
  config: {
    city: { type: "string", displayName: "City", default: "New York" },
    units: { type: "enum", displayName: "Units", values: ["Celsius", "Fahrenheit"], default: "Celsius" },
  },
  entryFields: { temperature: "number", condition: "string" },
  render: (entry) => Text(`${entry.temperature}° ${entry.condition}`),
});
```

## Provider Function

The `provider` field defines a timeline provider that fetches data for the widget:

```typescript,no-test
Widget({
  kind: "StockWidget",
  displayName: "Stock Price",
  description: "Shows current stock price",
  config: { symbol: { type: "string", displayName: "Symbol", default: "AAPL" } },
  entryFields: { price: "number", change: "string" },
  provider: async (config) => {
    const res = await fetch(`https://api.example.com/stock/${config.symbol}`);
    const data = await res.json();
    return { price: data.price, change: data.change };
  },
  render: (entry) =>
    VStack([
      Text(`$${entry.price}`).font("title"),
      Text(entry.change).color("green"),
    ]),
});
```

### Placeholder Data

When the widget has no data yet (e.g., first load), the provider can return placeholder data by providing a `placeholder` field:

```typescript,no-test
Widget({
  kind: "NewsWidget",
  entryFields: { headline: "string", source: "string" },
  placeholder: { headline: "Loading...", source: "---" },
  // ...
});
```

## Family-Specific Rendering

The render function accepts an optional second parameter for the widget family, allowing different layouts per size:

```typescript,no-test
render: (entry, family) =>
  family === "systemLarge"
    ? VStack([
        Text(entry.title).font("title"),
        ForEach(entry.items, (item) => Text(item.name)),
      ])
    : HStack([
        Image("star.fill"),
        Text(entry.title).font("headline"),
      ]),
```

Supported families: `"systemSmall"`, `"systemMedium"`, `"systemLarge"`, `"accessoryCircular"`, `"accessoryRectangular"`, `"accessoryInline"`.

## App Group

The `appGroup` field specifies a shared container for data exchange between the host app and the widget:

```typescript,no-test
Widget({
  kind: "AppDataWidget",
  appGroup: "group.com.example.myapp",
  // ...
});
```

## Multiple Widgets

Define multiple widgets in a single file. They're bundled into a `WidgetBundle`:

```typescript,no-test
Widget({
  kind: "SmallWidget",
  // ...
});

Widget({
  kind: "LargeWidget",
  // ...
});
```

## Next Steps

- [Components](components.md) — Available widget components and modifiers
- [Overview](overview.md) — Widget system overview
