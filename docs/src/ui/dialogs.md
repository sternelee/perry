# Dialogs

Perry provides native dialog functions for file selection, alerts, and sheets.

## File Open Dialog

```typescript,no-test
import { openFileDialog } from "perry/ui";

const filePath = openFileDialog();
if (filePath) {
  console.log(`Selected: ${filePath}`);
}
```

Returns the selected file path, or `null` if cancelled.

## Folder Selection Dialog

```typescript,no-test
import { openFolderDialog } from "perry/ui";

const folderPath = openFolderDialog();
if (folderPath) {
  console.log(`Selected folder: ${folderPath}`);
}
```

## Save File Dialog

```typescript,no-test
import { saveFileDialog } from "perry/ui";

const savePath = saveFileDialog();
if (savePath) {
  // Write file to savePath
}
```

## Alert

Display a native alert dialog:

```typescript,no-test
import { alert } from "perry/ui";

alert("Operation Complete", "Your file has been saved successfully.");
```

`alert(title, message)` shows a modal alert with an OK button.

## Sheets

Sheets are modal panels attached to a window:

```typescript,no-test
import { Sheet, Text, Button, VStack } from "perry/ui";

const sheet = Sheet(
  VStack(16, [
    Text("Sheet Content"),
    Button("Close", () => {
      sheet.dismiss();
    }),
  ])
);

// Show the sheet
sheet.present();
```

## Platform Notes

| Dialog | macOS | iOS | Windows | Linux | Web |
|--------|-------|-----|---------|-------|-----|
| File Open | NSOpenPanel | UIDocumentPicker | IFileOpenDialog | GtkFileChooserDialog | `<input type="file">` |
| File Save | NSSavePanel | — | IFileSaveDialog | GtkFileChooserDialog | Download link |
| Folder | NSOpenPanel | — | IFileOpenDialog | GtkFileChooserDialog | — |
| Alert | NSAlert | UIAlertController | MessageBoxW | MessageDialog | `alert()` |
| Sheet | NSSheet | Modal VC | Modal Dialog | Modal Window | Modal div |

## Complete Example

```typescript,no-test
import { App, Text, Button, TextField, VStack, HStack, State, openFileDialog, saveFileDialog, alert } from "perry/ui";
import { readFileSync, writeFileSync } from "perry/fs";

const content = State("");
const filePath = State("");

App({
  title: "Text Editor",
  width: 800,
  height: 600,
  body: VStack(12, [
    HStack(8, [
      Button("Open", () => {
        const path = openFileDialog();
        if (path) {
          filePath.set(path);
          content.set(readFileSync(path));
        }
      }),
      Button("Save As", () => {
        const path = saveFileDialog();
        if (path) {
          writeFileSync(path, content.value);
          filePath.set(path);
          alert("Saved", `File saved to ${path}`);
        }
      }),
    ]),
    Text(`File: ${filePath.value || "No file open"}`),
    TextField("Start typing...", (value: string) => content.set(value)),
  ]),
});
```

## Next Steps

- [Menus](menus.md) — Menu bar and context menus
- [Multi-Window](multi-window.md) — Multiple windows
- [Events](events.md) — User interaction events
