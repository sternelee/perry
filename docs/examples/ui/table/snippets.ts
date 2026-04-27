// demonstrates: per-API table snippets shown in docs/src/ui/table.md
// docs: docs/src/ui/table.md
// platforms: macos, linux, windows
// run: false

// `run: false` because the runnable behavior of Table — visible rows,
// selection events — needs an attached AppKit run loop to verify, and
// the doc-tests harness exits after 500 ms. Compile-link is enough to
// certify the codegen surface; on macOS the linked binary calls into
// the real NSTableView impl, on other platforms the no-op stubs.

import {
    App, VStack, Text,
    State,
    Table,
    tableSetColumnHeader, tableSetColumnWidth,
    tableSetOnRowSelect, tableGetSelectedRow,
    tableUpdateRowCount,
} from "perry/ui"

// ANCHOR: basic-table
const basicTable = Table(10, 3, (row: number, col: number) => {
    return Text(`Row ${row}, Col ${col}`)
})
// ANCHOR_END: basic-table

interface UserRow {
    name: string
    email: string
    role: string
}

const users: UserRow[] = [
    { name: "Alice",   email: "alice@example.com",   role: "Admin"  },
    { name: "Bob",     email: "bob@example.com",     role: "Editor" },
    { name: "Charlie", email: "charlie@example.com", role: "Viewer" },
    { name: "Diana",   email: "diana@example.com",   role: "Admin"  },
    { name: "Eve",     email: "eve@example.com",     role: "Editor" },
]

// ANCHOR: column-headers
const userTable = Table(users.length, 3, (row: number, col: number) => {
    const user = users[row]
    if (col === 0) return Text(user.name)
    if (col === 1) return Text(user.email)
    return Text(user.role)
})

tableSetColumnHeader(userTable, 0, "Name")
tableSetColumnHeader(userTable, 1, "Email")
tableSetColumnHeader(userTable, 2, "Role")
// ANCHOR_END: column-headers

// ANCHOR: column-widths
tableSetColumnWidth(userTable, 0, 150)  // Name column
tableSetColumnWidth(userTable, 1, 250)  // Email column
tableSetColumnWidth(userTable, 2, 100)  // Role column
// ANCHOR_END: column-widths

// ANCHOR: row-selection
const selectedRow = State(-1)

tableSetOnRowSelect(userTable, (row: number) => {
    selectedRow.set(row)
    console.log(`Selected row: ${row}`)
})

// Read the currently selected row at any time:
const current = tableGetSelectedRow(userTable)
// ANCHOR_END: row-selection

// ANCHOR: dynamic-rows
tableUpdateRowCount(userTable, users.length)
// ANCHOR_END: dynamic-rows

// ANCHOR: complete-example
const selectedName = State("None")

const table = Table(users.length, 3, (row: number, col: number) => {
    const user = users[row]
    if (col === 0) return Text(user.name)
    if (col === 1) return Text(user.email)
    return Text(user.role)
})

tableSetColumnHeader(table, 0, "Name")
tableSetColumnHeader(table, 1, "Email")
tableSetColumnHeader(table, 2, "Role")
tableSetColumnWidth(table, 0, 150)
tableSetColumnWidth(table, 1, 250)
tableSetColumnWidth(table, 2, 100)

tableSetOnRowSelect(table, (row: number) => {
    selectedName.set(users[row].name)
})

App({
    title: "Table Demo",
    width: 600,
    height: 400,
    body: VStack(12, [
        table,
        Text(`Selected: ${selectedName.value}`),
    ]),
})
// ANCHOR_END: complete-example

// Reference each name once so the linker doesn't dead-strip the FFIs.
console.log(`refs: basic=${basicTable} userTable=${userTable} current=${current}`)
