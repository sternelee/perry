# Layout

Perry provides layout containers that arrange child widgets using the platform's native layout system.

## VStack

Arranges children vertically (top to bottom).

```typescript,no-test
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

```typescript,no-test
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

```typescript,no-test
import { ZStack, Text, Image } from "perry/ui";

ZStack(0, [
  Image("background.png"),
  Text("Overlay text"),
]);
```

## ScrollView

A scrollable container.

```typescript,no-test
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

```typescript,no-test
import { LazyVStack, Text } from "perry/ui";

LazyVStack(1000, (index) => {
  return Text(`Row ${index}`);
});
```

## NavigationStack

A navigation container that supports push/pop navigation.

```typescript,no-test
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

```typescript,no-test
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

```typescript,no-test
import { VStack, Text, Divider } from "perry/ui";

VStack(12, [
  Text("Section 1"),
  Divider(),
  Text("Section 2"),
]);
```

## Nesting Layouts

Layouts can be nested freely:

```typescript,no-test
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

```typescript,no-test
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

## Stack Alignment

Control how children are aligned within a stack using `stackSetAlignment`:

```typescript,no-test
import { VStack, Text, stackSetAlignment } from "perry/ui";

const centered = VStack(16, [
  Text("Centered"),
  Text("Content"),
]);
stackSetAlignment(centered, 9); // CenterX
```

**VStack alignment** (cross-axis = horizontal):

| Value | Name | Effect |
|-------|------|--------|
| 5 | Leading | Children align to the leading (left) edge |
| 9 | CenterX | Children centered horizontally |
| 7 | Width | Children stretch to fill the stack's width |

**HStack alignment** (cross-axis = vertical):

| Value | Name | Effect |
|-------|------|--------|
| 3 | Top | Children align to the top |
| 12 | CenterY | Children centered vertically |
| 4 | Bottom | Children align to the bottom |

## Stack Distribution

Control how children share space within a stack using `stackSetDistribution`:

```typescript,no-test
import { HStack, Button, stackSetDistribution } from "perry/ui";

const buttons = HStack(8, [
  Button("Cancel", () => {}),
  Button("OK", () => {}),
]);
stackSetDistribution(buttons, 1); // FillEqually — both buttons get equal width
```

| Value | Name | Behavior |
|-------|------|----------|
| 0 | Fill | Default. First resizable child fills remaining space |
| 1 | FillEqually | All children get equal size |
| 2 | FillProportionally | Children sized proportionally to their intrinsic content |
| 3 | EqualSpacing | Equal gaps between children |
| 4 | EqualCentering | Equal distance between child centers |

## Fill Parent

Pin a child's edges to its parent container:

```typescript,no-test
import { VStack, Text, widgetMatchParentWidth } from "perry/ui";

const banner = Text("Full width banner");
widgetMatchParentWidth(banner);

VStack(16, [banner, Text("Normal width")]);
```

- `widgetMatchParentWidth(widget)` — stretch to fill parent's width
- `widgetMatchParentHeight(widget)` — stretch to fill parent's height

## Content Hugging

Control whether a widget resists being stretched beyond its intrinsic size:

```typescript,no-test
import { VStack, Text, widgetSetHugging } from "perry/ui";

const label = Text("I stay small");
widgetSetHugging(label, 750); // High priority — resist stretching

const filler = Text("I stretch");
widgetSetHugging(filler, 1); // Low priority — stretch to fill
```

- **High priority** (250-750+): widget resists stretching, stays at its natural size
- **Low priority** (1-249): widget stretches to fill available space

## Overlay Positioning

For absolute positioning, add overlay children to any container:

```typescript,no-test
import { VStack, Text, widgetAddOverlay, widgetSetOverlayFrame } from "perry/ui";

const container = VStack(16, [Text("Main content")]);

const badge = Text("3");
badge.setCornerRadius(10);
badge.setBackgroundColor("#FF3B30");

widgetAddOverlay(container, badge);
widgetSetOverlayFrame(badge, 280, 10, 20, 20); // x, y, width, height
```

Overlay children are positioned absolutely relative to their parent — similar to CSS `position: absolute`.

## Split Views

Create resizable split panes for sidebar layouts:

```typescript,no-test
import { SplitView, splitViewAddChild, VStack, Text } from "perry/ui";

const split = SplitView();

const sidebar = VStack(8, [Text("Navigation"), Text("Item 1"), Text("Item 2")]);
const content = VStack(16, [Text("Main Content")]);

splitViewAddChild(split, sidebar);
splitViewAddChild(split, content);
```

The user can drag the divider to resize panes. On macOS this maps to `NSSplitView`.

## Stacks with Built-in Padding

Create a stack with padding in a single call:

```typescript,no-test
import { VStackWithInsets, HStackWithInsets, Text, widgetAddChild } from "perry/ui";

// VStackWithInsets(spacing, top, right, bottom, left)
const card = VStackWithInsets(12, 16, 16, 16, 16);
widgetAddChild(card, Text("Padded content"));
widgetAddChild(card, Text("More content"));
```

Equivalent to creating a stack and then calling `setEdgeInsets`, but more concise. Children are added via `widgetAddChild` instead of the constructor array.

## Detaching Hidden Views

By default, hidden children still occupy space in a stack. To collapse them:

```typescript,no-test
import { VStack, Text, widgetSetHidden, stackSetDetachesHidden } from "perry/ui";

const stack = VStack(8, [Text("Always visible"), Text("Sometimes hidden")]);
stackSetDetachesHidden(stack, 1); // Hidden children leave no gap
```

## Common Layout Patterns

### Centered content

```typescript,no-test
const page = VStack(16, [Text("Title"), Text("Subtitle")]);
stackSetAlignment(page, 9); // CenterX
```

### Sidebar + content

```typescript,no-test
const split = SplitView();
splitViewAddChild(split, sidebar);
splitViewAddChild(split, content);
```

### Equal-width button row

```typescript,no-test
const row = HStack(8, [Button("Cancel", onCancel), Button("OK", onOK)]);
stackSetDistribution(row, 1); // FillEqually
```

### Full-width child in a stack

```typescript,no-test
const input = TextField("Search...", onChange);
widgetMatchParentWidth(input);
VStack(12, [input, results]);
```

### Floating badge / overlay

```typescript,no-test
const icon = Image("bell.png");
const badge = Text("3");
widgetAddOverlay(icon, badge);
widgetSetOverlayFrame(badge, 20, -5, 16, 16);
```

### Toolbar with spacer

```typescript,no-test
HStack(8, [
  Button("Back", goBack),
  Spacer(),
  Text("Page Title"),
  Spacer(),
  Button("Settings", openSettings),
]);
```

## Next Steps

- [Styling](styling.md) — Colors, padding, sizing
- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — Dynamic UI with state
