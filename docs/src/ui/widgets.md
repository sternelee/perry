# Widgets

Perry provides native widgets that map to each platform's native controls.

## Text

Displays read-only text.

```typescript,no-test
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

```typescript,no-test
const count = State(0);
Text(`Count: ${count.value}`); // Updates when count changes
```

## Button

A clickable button.

```typescript,no-test
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

```typescript,no-test
import { TextField, State, stateBindTextfield } from "perry/ui";

const text = State("");
const field = TextField("Placeholder...", (value: string) => text.set(value));
stateBindTextfield(text, field); // optional: let text.set(...) drive the field
```

`TextField(placeholder, onChange)` creates the field. The `onChange` callback fires
as the user types. For two-way binding (so programmatic `state.set(...)` also
updates the visible text) pair it with `stateBindTextfield(state, field)`.

## SecureField

A password input field (text is masked).

```typescript,no-test
import { SecureField, State } from "perry/ui";

const password = State("");
SecureField("Enter password...", (value: string) => password.set(value));
```

Same signature as `TextField` but input is hidden.

## Toggle

A boolean on/off switch.

```typescript,no-test
import { Toggle, State } from "perry/ui";

const enabled = State(false);
Toggle("Enable notifications", (on: boolean) => enabled.set(on));
```

## Slider

A numeric slider.

```typescript,no-test
import { Slider, State } from "perry/ui";

const value = State(50);
Slider(0, 100, (v: number) => value.set(v)); // min, max, onChange
```

## Picker

A dropdown selection control. Items are added with `pickerAddItem`.

```typescript,no-test
import { Picker, State, pickerAddItem } from "perry/ui";

const selected = State(0);
const picker = Picker((index: number) => selected.set(index));
pickerAddItem(picker, "Option A");
pickerAddItem(picker, "Option B");
pickerAddItem(picker, "Option C");
```

## Image

Displays an image.

```typescript,no-test
import { Image } from "perry/ui";

const img = Image("path/to/image.png");
img.setWidth(200);
img.setHeight(150);
```

On macOS/iOS, you can also use SF Symbol names:

```typescript,no-test
Image("star.fill"); // SF Symbol
```

## ProgressView

An indeterminate or determinate progress indicator.

```typescript,no-test
import { ProgressView } from "perry/ui";

const progress = ProgressView();
// Or with a value (0.0 to 1.0)
const progress = ProgressView(0.5);
```

## Form and Section

Group controls into a form layout.

```typescript,no-test
import { Form, Section, TextField, Toggle, State } from "perry/ui";

const name = State("");
const notifications = State(true);

Form([
  Section("Personal Info", [
    TextField("Name", (value: string) => name.set(value)),
  ]),
  Section("Settings", [
    Toggle("Notifications", (on: boolean) => notifications.set(on)),
  ]),
]);
```

## Table

A data table with rows and columns.

```typescript,no-test
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

```typescript,no-test
import { TextArea, State } from "perry/ui";

const content = State("");
TextArea(content, "Enter text...");
```

**Methods:**
- `setText(text: string)` — Set the text programmatically
- `getText()` — Get the current text

## QRCode

Generates and displays a QR code.

```typescript,no-test
import { QRCode } from "perry/ui";

const qr = QRCode("https://example.com", 200); // data, size
qr.setData("https://other-url.com");            // Update data
```

## Canvas

A drawing surface. See [Canvas](canvas.md) for the full drawing API.

```typescript,no-test
import { Canvas } from "perry/ui";

const canvas = Canvas(400, 300, (ctx) => {
  ctx.fillRect(10, 10, 100, 100);
  ctx.strokeRect(50, 50, 100, 100);
});
```

## CameraView

A live camera preview with color sampling. See [Camera](camera.md) for the full API.

```typescript,no-test
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
