# Styling

Perry widgets support native styling properties that map to each platform's styling system.

## Coming from CSS

Perry's layout model is closer to SwiftUI or Flutter than CSS. If you're coming from web development, here's how concepts translate:

| CSS | Perry |
|-----|-------|
| `display: flex; flex-direction: column` | `VStack(spacing, [...])` |
| `display: flex; flex-direction: row` | `HStack(spacing, [...])` |
| `justify-content` | `stackSetDistribution(stack, mode)` + `Spacer()` |
| `align-items` | `stackSetAlignment(stack, value)` |
| `position: absolute` | `widgetAddOverlay` + `widgetSetOverlayFrame` |
| `width: 100%` | `widgetMatchParentWidth(widget)` |
| `padding: 10px 20px` | `setEdgeInsets(10, 20, 10, 20)` |
| `gap: 16px` | `VStack(16, [...])` — first argument is the gap |
| CSS variables / design tokens | `perry-styling` package ([Theming](theming.md)) |
| `opacity` | `setOpacity(value)` |
| `border-radius` | `setCornerRadius(value)` |

See [Layout](layout.md) for full details on alignment, distribution, overlays, and split views.

## Colors

```typescript,no-test
import { Text, Button } from "perry/ui";

const label = Text("Colored text");
label.setColor("#FF0000");              // Text color (hex)
label.setBackgroundColor("#F0F0F0");    // Background color
```

Colors are specified as hex strings (`#RRGGBB`).

## Fonts

```typescript,no-test
const label = Text("Styled text");
label.setFontSize(24);                // Font size in points
label.setFontFamily("Menlo");         // Font family name
```

Use `"monospaced"` for the system monospaced font.

## Corner Radius

```typescript,no-test
const btn = Button("Rounded", () => {});
btn.setCornerRadius(12);
```

## Borders

```typescript,no-test
const widget = VStack(0, []);
widget.setBorderColor("#CCCCCC");
widget.setBorderWidth(1);
```

## Padding and Insets

```typescript,no-test
const stack = VStack(8, [Text("Padded content")]);
stack.setPadding(16);
stack.setEdgeInsets(10, 20, 10, 20); // top, right, bottom, left
```

## Sizing

```typescript,no-test
const widget = VStack(0, []);
widget.setWidth(300);
widget.setHeight(200);
widget.setFrame(0, 0, 300, 200);  // x, y, width, height
```

## Opacity

```typescript,no-test
const widget = Text("Semi-transparent");
widget.setOpacity(0.5); // 0.0 to 1.0
```

## Background Gradient

```typescript,no-test
const widget = VStack(0, []);
widget.setBackgroundGradient("#FF0000", "#0000FF"); // Start color, end color
```

## Control Size

```typescript,no-test
const btn = Button("Small", () => {});
btn.setControlSize(0); // 0=mini, 1=small, 2=regular, 3=large
```

> **macOS**: Maps to `NSControl.ControlSize`. Other platforms may interpret differently.

## Tooltips

```typescript,no-test
const btn = Button("Hover me", () => {});
btn.setTooltip("Click to perform action");
```

> **macOS/Windows/Linux**: Native tooltips. **iOS/Android**: No tooltip support. **Web**: HTML `title` attribute.

## Enabled/Disabled

```typescript,no-test
const btn = Button("Submit", () => {});
btn.setEnabled(false);  // Greys out and disables interaction
```

## Complete Styling Example

```typescript,no-test
import { App, Text, Button, VStack, HStack, State, Spacer } from "perry/ui";

const count = State(0);

const title = Text("Counter");
title.setFontSize(28);
title.setColor("#1A1A1A");

const display = Text(`${count.value}`);
display.setFontSize(48);
display.setFontFamily("monospaced");
display.setColor("#007AFF");

const decBtn = Button("-", () => count.set(count.value - 1));
decBtn.setCornerRadius(20);
decBtn.setBackgroundColor("#FF3B30");

const incBtn = Button("+", () => count.set(count.value + 1));
incBtn.setCornerRadius(20);
incBtn.setBackgroundColor("#34C759");

const controls = HStack(8, [decBtn, Spacer(), incBtn]);
controls.setPadding(20);

const container = VStack(16, [title, display, controls]);
container.setPadding(40);
container.setCornerRadius(16);
container.setBackgroundColor("#FFFFFF");
container.setBorderColor("#E5E5E5");
container.setBorderWidth(1);

App({
  title: "Styled App",
  width: 400,
  height: 300,
  body: container,
});
```

## Composing Styles

Reduce repetition by creating helper functions:

```typescript,no-test
import { VStackWithInsets, Text, widgetAddChild } from "perry/ui";

function card(children: any[]) {
  const c = VStackWithInsets(12, 16, 16, 16, 16);
  c.setCornerRadius(12);
  c.setBackgroundColor("#FFFFFF");
  c.setBorderColor("#E5E5E5");
  c.setBorderWidth(1);
  for (const child of children) widgetAddChild(c, child);
  return c;
}

// Usage
card([Text("Title"), Text("Body text")]);
```

For larger apps, use the `perry-styling` package to define design tokens in JSON and generate a typed theme file. See [Theming](theming.md) for the full workflow.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
- [Animation](animation.md) — Animate style changes
