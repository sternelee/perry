# Menus

Perry supports native menu bars, context menus, and toolbar items across all platforms.

## Menu Bar

Create a native application menu bar:

```typescript
import { App, VStack, Text, menuBarCreate, menuBarAddMenu, menuAddItem, menuAddSeparator, menuAddSubmenu, menuBarAttach } from "perry/ui";

// Build the menu bar before App(...)
const menuBar = menuBarCreate();

// File menu
const fileMenu = menuBarAddMenu(menuBar, "File");
menuAddItem(fileMenu, "New", () => newDoc(), "n");         // Cmd+N
menuAddItem(fileMenu, "Open", () => openDoc(), "o");       // Cmd+O
menuAddSeparator(fileMenu);
menuAddItem(fileMenu, "Save", () => saveDoc(), "s");       // Cmd+S
menuAddItem(fileMenu, "Save As...", () => saveAs(), "S");  // Cmd+Shift+S

// Edit menu
const editMenu = menuBarAddMenu(menuBar, "Edit");
menuAddItem(editMenu, "Undo", () => undo(), "z");
menuAddItem(editMenu, "Redo", () => redo(), "Z");         // Cmd+Shift+Z
menuAddSeparator(editMenu);
menuAddItem(editMenu, "Cut", () => cut(), "x");
menuAddItem(editMenu, "Copy", () => copy(), "c");
menuAddItem(editMenu, "Paste", () => paste(), "v");

// Submenu
const viewMenu = menuBarAddMenu(menuBar, "View");
const zoomSubmenu = menuAddSubmenu(viewMenu, "Zoom");
menuAddItem(zoomSubmenu, "Zoom In", () => zoomIn(), "+");
menuAddItem(zoomSubmenu, "Zoom Out", () => zoomOut(), "-");
menuAddItem(zoomSubmenu, "Actual Size", () => zoomReset(), "0");

menuBarAttach(menuBar);

App({
  title: "Menu Demo",
  width: 800,
  height: 600,
  body: VStack(16, [
    Text("App content here"),
  ]),
});
```

### Menu Bar Functions

- `menuBarCreate()` — Create a new menu bar
- `menuBarAddMenu(menuBar, title)` — Add a top-level menu, returns menu handle
- `menuAddItem(menu, label, callback, shortcut?)` — Add a menu item with optional keyboard shortcut
- `menuAddSeparator(menu)` — Add a separator line
- `menuAddSubmenu(menu, title)` — Add a submenu, returns submenu handle
- `menuBarAttach(menuBar)` — Attach the menu bar to the application

### Keyboard Shortcuts

The 4th argument to `menuAddItem` is an optional keyboard shortcut:

| Shortcut | macOS | Other |
|----------|-------|-------|
| `"n"` | Cmd+N | Ctrl+N |
| `"S"` | Cmd+Shift+S | Ctrl+Shift+S |
| `"+"` | Cmd++ | Ctrl++ |

Uppercase letters imply Shift.

## Context Menus

Right-click menus on widgets:

```typescript
import { Text, contextMenu } from "perry/ui";

const label = Text("Right-click me");
contextMenu(label, [
  { label: "Copy", action: () => copyText() },
  { label: "Paste", action: () => pasteText() },
  { separator: true },
  { label: "Delete", action: () => deleteItem() },
]);
```

## Toolbar

Add a toolbar to the window:

```typescript
import { App, VStack, Text, toolbarCreate, toolbarAddItem } from "perry/ui";

const toolbar = toolbarCreate();
toolbarAddItem(toolbar, "New", () => newDoc());
toolbarAddItem(toolbar, "Save", () => saveDoc());
toolbarAddItem(toolbar, "Run", () => runCode());

App({
  title: "Toolbar Demo",
  width: 800,
  height: 600,
  body: VStack(16, [
    Text("App content here"),
  ]),
});
```

## Platform Notes

| Platform | Menu Bar | Context Menu | Toolbar |
|----------|----------|-------------|---------|
| macOS | NSMenu | NSMenu | NSToolbar |
| iOS | — (no menu bar) | UIMenu | UIToolbar |
| Windows | HMENU/SetMenu | — | Horizontal layout |
| Linux | GMenu/set_menubar | — | HeaderBar |
| Web | DOM | DOM | DOM |

> **iOS**: Menu bars are not applicable. Use toolbar and navigation patterns instead.

## Next Steps

- [Events](events.md) — Keyboard shortcuts and interactions
- [Dialogs](dialogs.md) — File dialogs and alerts
- [Toolbar and navigation](layout.md)
