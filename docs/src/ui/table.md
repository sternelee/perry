# Table

The `Table` widget displays tabular data with columns, headers, and row selection.

## Creating a Table

```typescript,no-test
import { Table } from "perry/ui";

const table = Table(10, 3, (row, col) => {
  return `Row ${row}, Col ${col}`;
});
```

`Table(rowCount, colCount, renderCell)` creates a table. The render function is called for each cell and should return the text content.

## Column Headers

```typescript,no-test
const table = Table(100, 3, (row, col) => {
  const data = [
    ["Alice", "alice@example.com", "Admin"],
    ["Bob", "bob@example.com", "User"],
    // ...
  ];
  return data[row]?.[col] ?? "";
});

table.setColumnHeader(0, "Name");
table.setColumnHeader(1, "Email");
table.setColumnHeader(2, "Role");
```

## Column Widths

```typescript,no-test
table.setColumnWidth(0, 150);  // Name column
table.setColumnWidth(1, 250);  // Email column
table.setColumnWidth(2, 100);  // Role column
```

## Row Selection

```typescript,no-test
table.setOnRowSelect((row) => {
  console.log(`Selected row: ${row}`);
});

// Get the currently selected row
const selected = table.getSelectedRow();
```

## Dynamic Row Count

Update the number of rows after creation:

```typescript,no-test
table.updateRowCount(newCount);
```

## Platform Notes

| Platform | Implementation |
|----------|---------------|
| macOS | NSTableView + NSScrollView |
| Web | HTML `<table>` |
| iOS/Android/Linux/Windows | Stubs (pending native implementation) |

## Complete Example

```typescript,no-test
import { App, Table, Text, VStack, State } from "perry/ui";

const selectedName = State("None");

const users = [
  { name: "Alice", email: "alice@example.com", role: "Admin" },
  { name: "Bob", email: "bob@example.com", role: "Editor" },
  { name: "Charlie", email: "charlie@example.com", role: "Viewer" },
  { name: "Diana", email: "diana@example.com", role: "Admin" },
  { name: "Eve", email: "eve@example.com", role: "Editor" },
];

const table = Table(users.length, 3, (row, col) => {
  const user = users[row];
  if (col === 0) return user.name;
  if (col === 1) return user.email;
  return user.role;
});

table.setColumnHeader(0, "Name");
table.setColumnHeader(1, "Email");
table.setColumnHeader(2, "Role");
table.setColumnWidth(0, 150);
table.setColumnWidth(1, 250);
table.setColumnWidth(2, 100);

table.setOnRowSelect((row) => {
  selectedName.set(users[row].name);
});

App({
  title: "Table Demo",
  width: 600,
  height: 400,
  body: VStack(12, [
    table,
    Text(`Selected: ${selectedName.value}`),
  ]),
});
```

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
- [Events](events.md) — Event handling
