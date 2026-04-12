# Layout

Perry provides layout containers that arrange child widgets using the platform's native layout system.

## VStack

Arranges children vertically (top to bottom).

```typescript
import { VStack, Text, Button } from "perry/ui";

VStack(16, [
  Text("First"),
  Text("Second"),
  Text("Third"),
]);
```

`VStack(spacing, children)` — the first argument is the gap in points between children.

**Methods:**
- `setPadding(padding: number)` — Set padding around all edges
- `setSpacing(spacing: number)` — Set spacing between children

## HStack

Arranges children horizontally (left to right).

```typescript
import { HStack, Text, Button, Spacer } from "perry/ui";

HStack(8, [
  Button("Cancel", () => {}),
  Spacer(),
  Button("OK", () => {}),
]);
```

`HStack(spacing, children)` — the first argument is the gap in points between children.

## ZStack

Layers children on top of each other (back to front).

```typescript
import { ZStack, Text, Image } from "perry/ui";

ZStack(0, [
  Image("background.png"),
  Text("Overlay text"),
]);
```

## ScrollView

A scrollable container.

```typescript
import { ScrollView, VStack, Text } from "perry/ui";

ScrollView(
  VStack(
    8,
    Array.from({ length: 100 }, (_, i) => Text(`Row ${i}`))
  )
);
```

**Methods:**
- `setRefreshControl(callback: () => void)` — Add pull-to-refresh (calls callback on pull)
- `endRefreshing()` — Stop the refresh indicator

## LazyVStack

A vertically scrolling list that lazily renders items. More efficient than `ScrollView` + `VStack` for large lists.

```typescript
import { LazyVStack, Text } from "perry/ui";

LazyVStack(1000, (index) => {
  return Text(`Row ${index}`);
});
```

## NavigationStack

A navigation container that supports push/pop navigation.

```typescript
import { NavigationStack, VStack, Text, Button } from "perry/ui";

NavigationStack(
  VStack(16, [
    Text("Home Screen"),
    Button("Go to Details", () => {
      // Push a new view
    }),
  ])
);
```

## Spacer

A flexible space that expands to fill available room.

```typescript
import { HStack, Text, Spacer } from "perry/ui";

HStack(8, [
  Text("Left"),
  Spacer(),
  Text("Right"),
]);
```

Use `Spacer()` inside `HStack` or `VStack` to push widgets apart.

## Divider

A visual separator line.

```typescript
import { VStack, Text, Divider } from "perry/ui";

VStack(12, [
  Text("Section 1"),
  Divider(),
  Text("Section 2"),
]);
```

## Nesting Layouts

Layouts can be nested freely:

```typescript
import { App, VStack, HStack, Text, Button, Spacer, Divider } from "perry/ui";

App({
  title: "Layout Example",
  width: 800,
  height: 600,
  body: VStack(16, [
    // Header
    HStack(8, [
      Text("My App"),
      Spacer(),
      Button("Settings", () => {}),
    ]),
    Divider(),
    // Content
    VStack(12, [
      Text("Welcome!"),
      HStack(8, [
        Button("Action 1", () => {}),
        Button("Action 2", () => {}),
      ]),
    ]),
    Spacer(),
    // Footer
    Text("v1.0.0"),
  ]),
});
```

## Child Management

Containers support dynamic child management:

```typescript
const stack = VStack(16, []);
// Add children dynamically
stack.addChild(Text("New child"));
stack.addChildAt(0, Text("Prepended"));
stack.removeChild(someWidget);
stack.reorderChild(widget, 2);
stack.clearChildren();
```

**Methods:**
- `addChild(widget)` — Append a child widget
- `addChildAt(index, widget)` — Insert a child at a specific position
- `removeChild(widget)` — Remove a child widget
- `reorderChild(widget, newIndex)` — Move a child to a new position
- `clearChildren()` — Remove all children

## Next Steps

- [Styling](styling.md) — Colors, padding, sizing
- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — Dynamic UI with state
