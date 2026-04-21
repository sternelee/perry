# First Native App

Perry compiles declarative TypeScript UI code to native platform widgets. No Electron, no WebView — real AppKit on macOS, UIKit on iOS, GTK4 on Linux, Win32 on Windows.

## A Simple Counter

Create `counter.ts`:

```typescript,no-test
import { App, Text, Button, VStack, State } from "perry/ui";

const count = State(0);

App({
  title: "My Counter",
  width: 400,
  height: 300,
  body: VStack(16, [
    Text(`Count: ${count.value}`),
    Button("Increment", () => count.set(count.value + 1)),
    Button("Reset", () => count.set(0)),
  ]),
});
```

Compile and run:

```bash
perry counter.ts -o counter
./counter
```

A native window opens with a label and two buttons. Clicking "Increment" updates the count in real-time.

## How It Works

- **`App({ title, width, height, body })`** — Creates a native application window. `body` is the root widget.
- **`State(initialValue)`** — Creates reactive state. `.value` reads, `.set(v)` writes and triggers UI updates.
- **`VStack(spacing, [...])`** — Vertical stack layout (like SwiftUI's VStack or CSS flexbox column). Spacing arg is optional.
- **`Text(string)`** — A text label. Template literals referencing `${state.value}` bind reactively.
- **`Button(label, onClick)`** — A native button with a click handler.

## A Todo App

```typescript,no-test
import {
  App, Text, Button, TextField, VStack, HStack, State, ForEach, Spacer,
} from "perry/ui";

const todos = State<string[]>([]);
const count = State(0); // ForEach iterates by index, so we keep a count in sync
const input = State("");

App({
  title: "Todo App",
  width: 480,
  height: 600,
  body: VStack(16, [
    HStack(8, [
      TextField("Add a todo...", (value: string) => input.set(value)),
      Button("Add", () => {
        const text = input.value;
        if (text.length > 0) {
          todos.set([...todos.value, text]);
          count.set(count.value + 1);
          input.set("");
        }
      }),
    ]),
    ForEach(count, (i: number) =>
      HStack(8, [
        Text(todos.value[i]),
        Spacer(),
        Button("Remove", () => {
          todos.set(todos.value.filter((_, idx) => idx !== i));
          count.set(count.value - 1);
        }),
      ])
    ),
  ]),
});
```

## Cross-Platform

The same code runs on all 6 platforms:

```bash
# macOS (default)
perry app.ts -o app
./app

# iOS Simulator
perry app.ts -o app --target ios-simulator

# Web (compiles to WebAssembly + DOM bridge in a self-contained HTML file)
perry app.ts -o app --target web   # alias: --target wasm
open app.html

# Other platforms
perry app.ts -o app --target windows
perry app.ts -o app --target linux
perry app.ts -o app --target android
```

Each target compiles to the platform's native widget toolkit. See [Platforms](../platforms/overview.md) for details.

## Adding Styling

```typescript,no-test
import { App, Text, Button, VStack, State } from "perry/ui";

const count = State(0);

App("Styled Counter", () => {
  const label = Text(`Count: ${count.get()}`);
  label.setFontSize(24);
  label.setColor("#333333");

  const btn = Button("Increment", () => count.set(count.get() + 1));
  btn.setCornerRadius(8);
  btn.setBackgroundColor("#007AFF");

  const stack = VStack([label, btn]);
  stack.setPadding(20);
  return stack;
});
```

See [Styling](../ui/styling.md) for all available style properties.

## Next Steps

- [Project Configuration](project-config.md) — Set up `package.json` for Perry projects
- [UI Overview](../ui/overview.md) — Complete guide to Perry's UI system
- [Widgets Reference](../ui/widgets.md) — All available widgets
- [State Management](../ui/state.md) — Reactive state and bindings
