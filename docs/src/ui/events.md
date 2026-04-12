# Events

Perry widgets support native event handlers for user interaction.

## onClick

```typescript
import { Button, Text } from "perry/ui";

Button("Click me", () => {
  console.log("Button clicked!");
});

// Or set it after creation
const label = Text("Clickable text");
label.setOnClick(() => {
  console.log("Text clicked!");
});
```

## onHover

Triggered when the mouse enters or leaves a widget.

```typescript
import { Button } from "perry/ui";

const btn = Button("Hover me", () => {});
btn.setOnHover((isHovering) => {
  if (isHovering) {
    console.log("Mouse entered");
  } else {
    console.log("Mouse left");
  }
});
```

> **Note**: Hover events are available on macOS, Windows, Linux, and Web. iOS and Android use touch interactions instead.

## onDoubleClick

```typescript
import { Text } from "perry/ui";

const label = Text("Double-click me");
label.setOnDoubleClick(() => {
  console.log("Double-clicked!");
});
```

## Keyboard Shortcuts

Register in-app keyboard shortcuts (active when the app is focused):

```typescript
import { addKeyboardShortcut } from "perry/ui";

// Cmd+N on macOS, Ctrl+N on other platforms
addKeyboardShortcut("n", 1, () => {
  console.log("New document");
});

// Cmd+Shift+S (modifiers: 1=Cmd/Ctrl, 2=Shift, 4=Option/Alt, 8=Control)
addKeyboardShortcut("s", 3, () => {
  console.log("Save as...");
});
```

Keyboard shortcuts are also supported in [menu items](menus.md):

```typescript
menuAddItem(menu, "New", () => newDoc(), "n");    // Cmd+N
menuAddItem(menu, "Save As", () => saveAs(), "S"); // Cmd+Shift+S
```

## Global Hotkeys

Register system-wide hotkeys that work even when the app is in the background — essential for launchers, clipboard managers, and quick-access tools:

```typescript
import { registerGlobalHotkey } from "perry/ui";

// Cmd+Space (macOS) / Ctrl+Space (Windows)
registerGlobalHotkey("space", 1, () => {
  // Show/hide your launcher
});

// Cmd+Shift+V (clipboard manager)
registerGlobalHotkey("v", 3, () => {
  // Show clipboard history
});
```

**Modifier bits:** `1` = Cmd (macOS) / Ctrl (Windows), `2` = Shift, `4` = Option (macOS) / Alt (Windows), `8` = Control (macOS only). Combine by adding: `3` = Cmd+Shift, `5` = Cmd+Option, etc.

| Platform | Implementation |
|----------|---------------|
| macOS | `NSEvent.addGlobalMonitorForEvents` + `addLocalMonitorForEvents` |
| Windows | `RegisterHotKey` + `WM_HOTKEY` dispatch in message loop |
| Linux | Not yet supported (requires X11 `XGrabKey` or Wayland portal) |

> **macOS note:** Global event monitoring requires accessibility permissions. The user will see a system prompt on first use.

> **Linux note:** Global hotkeys are a known limitation. On X11, `XGrabKey` is possible but not yet implemented. On Wayland, the `GlobalShortcuts` portal has limited compositor support.

## Clipboard

```typescript
import { clipboardGet, clipboardSet } from "perry/ui";

// Copy to clipboard
clipboardSet("Hello, clipboard!");

// Read from clipboard
const text = clipboardGet();
```

## Complete Example

```typescript
import { App, Text, Button, VStack, HStack, State, Spacer, registerShortcut } from "perry/ui";

const lastEvent = State("No events yet");

// Global shortcut
registerShortcut("r", () => {
  lastEvent.set("Keyboard: Cmd+R");
});

const hoverBtn = Button("Hover me", () => {});
hoverBtn.setOnHover((h) => {
  lastEvent.set(h ? "Mouse entered" : "Mouse left");
});

const dblLabel = Text("Double-click me");
dblLabel.setOnDoubleClick(() => {
  lastEvent.set("Double-clicked!");
});

App({
  title: "Events Demo",
  width: 400,
  height: 300,
  body: VStack(16, [
    Text(`Last event: ${lastEvent.value}`),

    Spacer(),

    Button("Click me", () => {
      lastEvent.set("Button clicked");
    }),

    hoverBtn,
    dblLabel,
  ]),
});
```

## Next Steps

- [Menus](menus.md) — Menu bar and context menus with keyboard shortcuts
- [Widgets](widgets.md) — All available widgets
- [State Management](state.md) — Reactive state
