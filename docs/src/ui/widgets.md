# Widgets

Perry provides native widgets that map to each platform's native controls.

## Text

Displays read-only text.

```typescript
import { Text } from "perry/ui";

const label = Text("Hello, World!");
label.setFontSize(18);
label.setColor("#333333");
label.setFontFamily("Menlo");
```

**Methods:**
- `setText(text: string)` — Update the text content
- `setFontSize(size: number)` — Set font size in points
- `setColor(hex: string)` — Set text color (hex string)
- `setFontFamily(family: string)` — Set font family (e.g., `"Menlo"` for monospaced)
- `setAccessibilityLabel(label: string)` — Set accessibility label

Text widgets inside template literals with `state.value` update automatically:

```typescript
const count = State(0);
Text(`Count: ${count.value}`); // Updates when count changes
```

## Button

A clickable button.

```typescript
import { Button } from "perry/ui";

const btn = Button("Click Me", () => {
  console.log("Clicked!");
});
btn.setCornerRadius(8);
btn.setBackgroundColor("#007AFF");
```

**Methods:**
- `setOnClick(callback: () => void)` — Set click handler
- `setImage(sfSymbolName: string)` — Set SF Symbol icon (macOS/iOS)
- `setContentTintColor(hex: string)` — Set tint color
- `setImagePosition(position: number)` — Set image position relative to text
- `setEnabled(enabled: boolean)` — Enable/disable the button
- `setAccessibilityLabel(label: string)` — Set accessibility label

## TextField

An editable text input.

```typescript
import { TextField, State } from "perry/ui";

const text = State("");
const field = TextField(text, "Placeholder...");
```

`TextField` takes a `State` for two-way binding — the state updates as the user types, and setting the state updates the field.

**Methods:**
- `setText(text: string)` — Set the text programmatically
- `setPlaceholder(text: string)` — Set placeholder text
- `setEnabled(enabled: boolean)` — Enable/disable editing

## SecureField

A password input field (text is masked).

```typescript
import { SecureField, State } from "perry/ui";

const password = State("");
SecureField(password, "Enter password...");
```

Same API as `TextField` but input is hidden.

## Toggle

A boolean on/off switch.

```typescript
import { Toggle, State } from "perry/ui";

const enabled = State(false);
Toggle("Enable notifications", enabled);
```

The `State` is bound two-way — toggling updates the state, and setting the state updates the toggle.

## Slider

A numeric slider.

```typescript
import { Slider, State } from "perry/ui";

const value = State(50);
Slider(value, 0, 100); // state, min, max
```

## Picker

A dropdown selection control.

```typescript
import { Picker, State } from "perry/ui";

const selected = State(0);
Picker(["Option A", "Option B", "Option C"], selected);
```

## Image

Displays an image.

```typescript
import { Image } from "perry/ui";

const img = Image("path/to/image.png");
img.setWidth(200);
img.setHeight(150);
```

On macOS/iOS, you can also use SF Symbol names:

```typescript
Image("star.fill"); // SF Symbol
```

## ProgressView

An indeterminate or determinate progress indicator.

```typescript
import { ProgressView } from "perry/ui";

const progress = ProgressView();
// Or with a value (0.0 to 1.0)
const progress = ProgressView(0.5);
```

## Form and Section

Group controls into a form layout.

```typescript
import { Form, Section, TextField, Toggle, State } from "perry/ui";

const name = State("");
const notifications = State(true);

Form([
  Section("Personal Info", [
    TextField(name, "Name"),
  ]),
  Section("Settings", [
    Toggle("Notifications", notifications),
  ]),
]);
```

## Table

A data table with rows and columns.

```typescript
import { Table } from "perry/ui";

const table = Table(10, 3, (row, col) => {
  return `Cell ${row},${col}`;
});

table.setColumnHeader(0, "Name");
table.setColumnHeader(1, "Email");
table.setColumnHeader(2, "Role");
table.setColumnWidth(0, 200);
table.setColumnWidth(1, 250);

table.setOnRowSelect((row) => {
  console.log(`Selected row: ${row}`);
});
```

**Methods:**
- `setColumnHeader(col: number, title: string)` — Set column header text
- `setColumnWidth(col: number, width: number)` — Set column width
- `updateRowCount(count: number)` — Update the number of rows
- `setOnRowSelect(callback: (row: number) => void)` — Row selection handler
- `getSelectedRow()` — Get currently selected row index

## TextArea

A multi-line text input.

```typescript
import { TextArea, State } from "perry/ui";

const content = State("");
TextArea(content, "Enter text...");
```

**Methods:**
- `setText(text: string)` — Set the text programmatically
- `getText()` — Get the current text

## QRCode

Generates and displays a QR code.

```typescript
import { QRCode } from "perry/ui";

const qr = QRCode("https://example.com", 200); // data, size
qr.setData("https://other-url.com");            // Update data
```

## Canvas

A drawing surface. See [Canvas](canvas.md) for the full drawing API.

```typescript
import { Canvas } from "perry/ui";

const canvas = Canvas(400, 300, (ctx) => {
  ctx.fillRect(10, 10, 100, 100);
  ctx.strokeRect(50, 50, 100, 100);
});
```

## CameraView

A live camera preview with color sampling. See [Camera](camera.md) for the full API.

```typescript
import { CameraView, cameraStart, cameraSampleColor, cameraSetOnTap } from "perry/ui";

const cam = CameraView();
cameraStart(cam);

cameraSetOnTap(cam, (x, y) => {
  const rgb = cameraSampleColor(x, y); // packed r*65536 + g*256 + b
});
```

> **iOS only.** Other platforms are planned.

**Functions:**
- `CameraView()` — Create a camera preview widget
- `cameraStart(handle)` — Start live capture
- `cameraStop(handle)` — Stop capture
- `cameraFreeze(handle)` — Pause preview (freeze frame)
- `cameraUnfreeze(handle)` — Resume preview
- `cameraSampleColor(x, y)` — Sample color at normalized coordinates (returns packed RGB or -1)
- `cameraSetOnTap(handle, callback)` — Register tap handler with `(x, y)` coordinates

## Common Widget Methods

All widgets support these methods:

| Method | Description |
|--------|-------------|
| `setWidth(width)` | Set width |
| `setHeight(height)` | Set height |
| `setBackgroundColor(hex)` | Set background color |
| `setCornerRadius(radius)` | Set corner radius |
| `setOpacity(alpha)` | Set opacity (0.0–1.0) |
| `setEnabled(enabled)` | Enable/disable interaction |
| `setHidden(hidden)` | Show/hide widget |
| `setTooltip(text)` | Set tooltip text |
| `setOnClick(callback)` | Set click handler |
| `setOnHover(callback)` | Set hover handler |
| `setOnDoubleClick(callback)` | Set double-click handler |

See [Styling](styling.md) and [Events](events.md) for complete details.

## Next Steps

- [Layout](layout.md) — Arranging widgets with stacks and containers
- [Styling](styling.md) — Colors, fonts, borders
- [State Management](state.md) — Reactive bindings
