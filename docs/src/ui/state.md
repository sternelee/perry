# State Management

Perry uses reactive state to automatically update the UI when data changes.

## Creating State

```typescript,no-test
import { State } from "perry/ui";

const count = State(0);           // number state
const name = State("Perry");      // string state
const items = State<string[]>([]); // array state
```

`State(initialValue)` creates a reactive state container.

## Reading and Writing

```typescript,no-test
const value = count.value;  // Read current value
count.set(42);              // Set new value → triggers UI update
```

Every `.set()` call re-renders the widget tree with the new value.

## Reactive Text

Template literals with `state.value` update automatically:

```typescript,no-test
import { Text, State } from "perry/ui";

const count = State(0);
Text(`Count: ${count.value}`);
// The text updates whenever count changes
```

This works because Perry detects `state.value` reads inside template literals and creates reactive bindings.

## Binding Inputs to State

Input widgets expose an `onChange` callback. Forward that into a state's
`.set(...)` to keep the state in sync as the user types/toggles/drags:

```typescript,no-test
import { TextField, State, stateBindTextfield } from "perry/ui";

const input = State("");
const field = TextField("Type here...", (value: string) => input.set(value));

// Optional: also let input.set("hello") update the field on screen.
stateBindTextfield(input, field);
```

Use `stateBindTextfield` whenever your code needs to clear or replace the field
programmatically. The fuller TODO example below uses it so pressing **Add** clears
both the state and the visible input box.

Input control signatures:
- `TextField(placeholder, onChange)` — text input, `onChange: (value: string) => void`
- `SecureField(placeholder, onChange)` — password input, `onChange: (value: string) => void`
- `Toggle(label, onChange)` — boolean toggle, `onChange: (value: boolean) => void`
- `Slider(min, max, onChange)` — numeric slider, `onChange: (value: number) => void`
- `Picker(onChange)` — dropdown, `onChange: (index: number) => void`; items via `pickerAddItem`

For programmatic-to-UI sync (state-drives-widget) use the dedicated binders:
`stateBindTextfield`, `stateBindSlider`, `stateBindToggle`, `stateBindTextNumeric`,
`stateBindVisibility`.

## onChange Callbacks

Listen for state changes with the free-function `stateOnChange`:

```typescript,no-test
import { State, stateOnChange } from "perry/ui";

const count = State(0);
stateOnChange(count, (newValue: number) => {
  console.log(`Count changed to ${newValue}`);
});
```

## ForEach

Render a list from numeric state (the index count):

```typescript,no-test
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
>
> `ForEach` listens to the count state, not to same-length edits inside the array. Add/remove flows should update both the array and the count; same-length per-row interactions are best handled by the row widgets themselves plus any separate summary state you want to update.

`ForEach` re-renders the list when the count state changes:

```typescript,no-test
// Add an item
items.set([...items.value, "Date"]);
itemCount.set(itemCount.value + 1);

// Remove an item
items.set(items.value.filter((_, i) => i !== 1));
itemCount.set(itemCount.value - 1);
```

## Conditional Rendering

Use state to conditionally show widgets:

```typescript,no-test
import { VStack, Text, Button, State } from "perry/ui";

const showDetails = State(false);

VStack(16, [
  Button("Toggle", () => showDetails.set(!showDetails.value)),
  showDetails.value ? Text("Details are visible!") : Spacer(),
]);
```

## Multi-State Text

Text can depend on multiple state values:

```typescript,no-test
const firstName = State("John");
const lastName = State("Doe");

Text(`Hello, ${firstName.value} ${lastName.value}!`);
// Updates when either firstName or lastName changes
```

## State with Objects and Arrays

```typescript,no-test
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
{{#include ../../examples/ui/state/todo_app_complete.ts}}
```

This fuller example keeps a separate `count` state for `ForEach`, uses
`stateBindTextfield` so **Add** visibly clears the field, and uses `Toggle`
(rather than a checkbox) for cross-platform completion state.

This program is built and run by CI (`scripts/run_doc_tests.sh`), so the
snippet above always matches the compiled artifact under
[`docs/examples/ui/state/todo_app_complete.ts`](../../examples/ui/state/todo_app_complete.ts).

If you want the smaller add/delete-only version, see
[`docs/examples/ui/state/todo_app.ts`](../../examples/ui/state/todo_app.ts).

## Next Steps

- [Events](events.md) — Click, hover, keyboard events
- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
