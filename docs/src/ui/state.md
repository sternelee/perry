# State Management

Perry uses reactive state to automatically update the UI when data changes.

## Creating State

```typescript
import { State } from "perry/ui";

const count = State(0);           // number state
const name = State("Perry");      // string state
const items = State<string[]>([]); // array state
```

`State(initialValue)` creates a reactive state container.

## Reading and Writing

```typescript
const value = count.value;  // Read current value
count.set(42);              // Set new value → triggers UI update
```

Every `.set()` call re-renders the widget tree with the new value.

## Reactive Text

Template literals with `state.value` update automatically:

```typescript
import { Text, State } from "perry/ui";

const count = State(0);
Text(`Count: ${count.value}`);
// The text updates whenever count changes
```

This works because Perry detects `state.value` reads inside template literals and creates reactive bindings.

## Two-Way Binding

`TextField` and other input widgets bind to state bidirectionally:

```typescript
import { TextField, State } from "perry/ui";

const input = State("");
TextField(input, "Type here...");

// input.value always reflects what the user typed
// input.set("hello") updates the text field
```

Controls that support two-way binding:
- `TextField(state, placeholder)` — text input
- `SecureField(state, placeholder)` — password input
- `Toggle(label, state)` — boolean toggle
- `Slider(state, min, max)` — numeric slider
- `Picker(options, state)` — selection

## onChange Callbacks

Listen for state changes:

```typescript
import { State } from "perry/ui";

const count = State(0);
count.onChange((newValue) => {
  console.log(`Count changed to ${newValue}`);
});
```

## ForEach

Render a list from numeric state (the index count):

```typescript
import { VStack, Text, ForEach, State } from "perry/ui";

const items = State(["Apple", "Banana", "Cherry"]);
const itemCount = State(3);

VStack(16, [
  ForEach(itemCount, (i: number) =>
    Text(`${i + 1}. ${items.value[i]}`)
  ),
]);
```

> **Note:** `ForEach` iterates by index over a numeric state. Keep a count state in sync with your array, then read the items via `array.value[i]` inside the closure.

`ForEach` re-renders the list when the count state changes:

```typescript
// Add an item
items.set([...items.value, "Date"]);
itemCount.set(itemCount.value + 1);

// Remove an item
items.set(items.value.filter((_, i) => i !== 1));
itemCount.set(itemCount.value - 1);
```

## Conditional Rendering

Use state to conditionally show widgets:

```typescript
import { VStack, Text, Button, State } from "perry/ui";

const showDetails = State(false);

VStack(16, [
  Button("Toggle", () => showDetails.set(!showDetails.value)),
  showDetails.value ? Text("Details are visible!") : Spacer(),
]);
```

## Multi-State Text

Text can depend on multiple state values:

```typescript
const firstName = State("John");
const lastName = State("Doe");

Text(`Hello, ${firstName.value} ${lastName.value}!`);
// Updates when either firstName or lastName changes
```

## State with Objects and Arrays

```typescript
const user = State({ name: "Perry", age: 0 });

// Update by replacing the whole object
user.set({ ...user.value, age: 1 });

const todos = State<{ text: string; done: boolean }[]>([]);

// Add a todo
todos.set([...todos.value, { text: "New task", done: false }]);

// Toggle a todo
const items = todos.value;
items[0].done = !items[0].done;
todos.set([...items]);
```

> **Note**: State uses identity comparison. You must create a new array/object reference for changes to be detected. Mutating in-place without calling `.set()` with a new reference won't trigger updates.

## Complete Example

```typescript
import { App, Text, Button, TextField, VStack, HStack, State, ForEach, Spacer, Divider } from "perry/ui";

const todos = State<string[]>([]);
const count = State(0);
const input = State("");

App({
  title: "Todo App",
  width: 480,
  height: 600,
  body: VStack(16, [
    Text("My Todos"),

    HStack(8, [
      TextField(input, "What needs to be done?"),
      Button("Add", () => {
        const text = input.value;
        if (text.length > 0) {
          todos.set([...todos.value, text]);
          count.set(count.value + 1);
          input.set("");
        }
      }),
    ]),

    Divider(),

    ForEach(count, (i: number) =>
      HStack(8, [
        Text(todos.value[i]),
        Spacer(),
        Button("Delete", () => {
          todos.set(todos.value.filter((_, idx) => idx !== i));
          count.set(count.value - 1);
        }),
      ])
    ),

    Spacer(),
    Text(`${count.value} items`),
  ]),
});
```

## Next Steps

- [Events](events.md) — Click, hover, keyboard events
- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
