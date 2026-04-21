# Styling

Perry widgets are styled through a set of **free functions** that take the
widget handle as the first argument. There are no instance-method setters on
the widget handle itself — `label.setColor(...)` and friends don't exist;
write `textSetColor(label, r, g, b, a)` instead.

The exact function surface lives in `types/perry/ui/index.d.ts` (installed by
`perry init`). This page shows the common shapes.

## Coming from CSS

Perry's layout model is closer to SwiftUI or Flutter than CSS. If you're
coming from web development, here's how concepts translate:

| CSS | Perry |
|-----|-------|
| `display: flex; flex-direction: column` | `VStack(spacing, [...])` |
| `display: flex; flex-direction: row` | `HStack(spacing, [...])` |
| `justify-content` | `stackSetDistribution(stack, mode)` + `Spacer()` |
| `align-items` | `stackSetAlignment(stack, value)` |
| `position: absolute` | `widgetAddOverlay(parent, child)` + `widgetSetOverlayFrame(child, x, y, w, h)` |
| `width: 100%` | `widgetMatchParentWidth(widget)` |
| `padding: 10px 20px` | `widgetSetEdgeInsets(widget, 10, 20, 10, 20)` |
| `gap: 16px` | `VStack(16, [...])` — first argument is the gap |
| CSS variables / design tokens | `perry-styling` package ([Theming](theming.md)) |
| `opacity` | `widgetSetOpacity(widget, value)` |
| `border-radius` | `setCornerRadius(widget, value)` |

See [Layout](layout.md) for full details on alignment, distribution,
overlays, and split views.

## Colors

Colors are passed as four RGBA floats in the `[0.0, 1.0]` range — not hex
strings. For opaque colors, use `1.0` as the alpha.

```typescript
import { Text, textSetColor, widgetSetBackgroundColor } from "perry/ui";

const label = Text("Colored text");
textSetColor(label, 1.0, 0.0, 0.0, 1.0);              // red text
widgetSetBackgroundColor(label, 0.94, 0.94, 0.94, 1.0); // #F0F0F0-ish
```

If you're used to hex, divide each byte by 255: `#007AFF` →
`widgetSetBackgroundColor(w, 0x00/255, 0x7A/255, 0xFF/255, 1.0)` =
`(0.0, 0.478, 1.0, 1.0)`.

## Fonts

```typescript
import { Text, textSetFontSize, textSetFontFamily, textSetFontWeight } from "perry/ui";

const label = Text("Styled text");
textSetFontSize(label, 24);             // size in points
textSetFontFamily(label, "Menlo");      // family name
textSetFontWeight(label, 24, 1.0);      // size + weight (0.0 thin … 1.0 bold)
```

Use `"monospaced"` for the system monospaced font.

## Corner Radius

```typescript
import { Button, setCornerRadius } from "perry/ui";

const btn = Button("Rounded", () => {});
setCornerRadius(btn, 12);
```

## Borders

```typescript
import { VStack, widgetSetBorderColor, widgetSetBorderWidth } from "perry/ui";

const card = VStack(0, []);
widgetSetBorderColor(card, 0.8, 0.8, 0.8, 1.0);  // #CCCCCC
widgetSetBorderWidth(card, 1);
```

## Padding and Insets

`setPadding` takes four numbers (top / left / bottom / right). To pad
symmetrically with a single value, pass it four times:

```typescript
import { VStack, Text, setPadding, widgetSetEdgeInsets } from "perry/ui";

const stack = VStack(8, [Text("Padded content")]);
setPadding(stack, 16, 16, 16, 16);
widgetSetEdgeInsets(stack, 10, 20, 10, 20); // top, left, bottom, right
```

## Sizing

```typescript
import { VStack, widgetSetWidth, widgetSetHeight } from "perry/ui";

const box = VStack(0, []);
widgetSetWidth(box, 300);
widgetSetHeight(box, 200);
```

## Opacity

```typescript
import { Text, widgetSetOpacity } from "perry/ui";

const ghost = Text("Semi-transparent");
widgetSetOpacity(ghost, 0.5); // 0.0 … 1.0
```

## Background Gradient

`widgetSetBackgroundGradient` takes two RGBA colors plus an angle in degrees:

```typescript
import { VStack, widgetSetBackgroundGradient } from "perry/ui";

const box = VStack(0, []);
widgetSetBackgroundGradient(
  box,
  1.0, 0.0, 0.0, 1.0, // start: red
  0.0, 0.0, 1.0, 1.0, // end:   blue
  90,                  // angle in degrees
);
```

## Control Size

```typescript
import { Button, widgetSetControlSize } from "perry/ui";

const btn = Button("Small", () => {});
widgetSetControlSize(btn, 0); // 0=mini, 1=small, 2=regular, 3=large
```

> **macOS**: Maps to `NSControl.ControlSize`. Other platforms may interpret
> differently.

## Tooltips

```typescript
import { Button, widgetSetTooltip } from "perry/ui";

const btn = Button("Hover me", () => {});
widgetSetTooltip(btn, "Click to perform action");
```

> **macOS/Windows/Linux**: Native tooltips. **iOS/Android**: No tooltip
> support. **Web**: HTML `title` attribute.

## Enabled / Disabled

`widgetSetEnabled` takes `1` for enabled and `0` for disabled:

```typescript
import { Button, widgetSetEnabled } from "perry/ui";

const btn = Button("Submit", () => {});
widgetSetEnabled(btn, 0); // greys out and disables interaction
```

## Complete Styling Example

```typescript
import {
  App, Text, Button, VStack, HStack, State, Spacer,
  textSetFontSize, textSetFontFamily, textSetColor,
  setCornerRadius, setPadding,
  widgetSetBackgroundColor, widgetSetBorderColor, widgetSetBorderWidth,
} from "perry/ui";

const count = State(0);

const title = Text("Counter");
textSetFontSize(title, 28);
textSetColor(title, 0.1, 0.1, 0.1, 1.0);

const display = Text(`${count.value}`);
textSetFontSize(display, 48);
textSetFontFamily(display, "monospaced");
textSetColor(display, 0.0, 0.48, 1.0, 1.0); // #007AFF

const decBtn = Button("-", () => count.set(count.value - 1));
setCornerRadius(decBtn, 20);
widgetSetBackgroundColor(decBtn, 1.0, 0.23, 0.19, 1.0); // #FF3B30

const incBtn = Button("+", () => count.set(count.value + 1));
setCornerRadius(incBtn, 20);
widgetSetBackgroundColor(incBtn, 0.20, 0.78, 0.35, 1.0); // #34C759

const controls = HStack(8, [decBtn, Spacer(), incBtn]);
setPadding(controls, 20, 20, 20, 20);

const container = VStack(16, [title, display, controls]);
setPadding(container, 40, 40, 40, 40);
setCornerRadius(container, 16);
widgetSetBackgroundColor(container, 1.0, 1.0, 1.0, 1.0);
widgetSetBorderColor(container, 0.9, 0.9, 0.9, 1.0); // #E5E5E5
widgetSetBorderWidth(container, 1);

App({
  title: "Styled App",
  width: 400,
  height: 300,
  body: container,
});
```

> **Note on `App(...)`**: the only supported form is `App(config)` where
> `config` is a `{ title, width, height, body, icon? }` object literal. There
> is **no** `App(title, builder)` callback form — if you write one, the
> compiler will refuse the program.

## Composing Styles

Reduce repetition by wrapping the free functions in helpers:

```typescript
import {
  VStackWithInsets, Text, widgetAddChild, Widget,
  setCornerRadius, widgetSetBackgroundColor, widgetSetBorderColor, widgetSetBorderWidth,
} from "perry/ui";

function card(children: Widget[]): Widget {
  const c = VStackWithInsets(12, 16, 16, 16, 16);
  setCornerRadius(c, 12);
  widgetSetBackgroundColor(c, 1.0, 1.0, 1.0, 1.0);
  widgetSetBorderColor(c, 0.9, 0.9, 0.9, 1.0);
  widgetSetBorderWidth(c, 1);
  for (const child of children) widgetAddChild(c, child);
  return c;
}

// Usage
card([Text("Title"), Text("Body text")]);
```

For larger apps, use the `perry-styling` package to define design tokens in
JSON and generate a typed theme file. See [Theming](theming.md) for the full
workflow.

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
- [Animation](animation.md) — Animate style changes
