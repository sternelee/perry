# Table

The `Table` widget displays tabular data with columns, headers, and row
selection.

> **Platform support:** real implementation lives on **macOS**
> (`NSTableView` + `NSScrollView`); the **Web** target uses an HTML
> `<table>`. **iOS**, **Android**, **Linux/GTK4**, **Windows**, **tvOS**,
> **visionOS**, and **watchOS** link no-op stubs so cross-platform code
> compiles everywhere — the table renders nothing and `tableGetSelectedRow`
> returns `-1`. For production lists on platforms without a real impl,
> use `LazyVStack` (see [Layout](layout.md)).

## Creating a Table

```ts
{{#include ../../examples/ui/table/snippets.ts:basic-table}}
```

`Table(rowCount, colCount, renderCell)` creates a table. The render
callback receives `(row, col)` and must return a `Widget` (typically
`Text(...)`). The runtime resolves the returned handle as the cell
view, which lets cells render images, stacks, or composites — not just
plain strings.

## Column Headers

```ts
{{#include ../../examples/ui/table/snippets.ts:column-headers}}
```

## Column Widths

```ts
{{#include ../../examples/ui/table/snippets.ts:column-widths}}
```

## Row Selection

```ts
{{#include ../../examples/ui/table/snippets.ts:row-selection}}
```

## Dynamic Row Count

Update the number of rows after creation:

```ts
{{#include ../../examples/ui/table/snippets.ts:dynamic-rows}}
```

## Complete Example

```ts
{{#include ../../examples/ui/table/snippets.ts:complete-example}}
```

## Next Steps

- [Widgets](widgets.md) — All available widgets
- [Layout](layout.md) — Layout containers
- [Events](events.md) — Event handling
