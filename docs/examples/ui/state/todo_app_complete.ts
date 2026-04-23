// demonstrates: fuller reactive todo app with add, delete, complete, clear-completed, and live stats
// docs: docs/src/ui/state.md
// platforms: macos, linux, windows
// targets: ios-simulator, web, wasm

import {
    App,
    Text,
    Button,
    TextField,
    Toggle,
    VStack,
    HStack,
    State,
    ForEach,
    Spacer,
    Divider,
    stateBindTextfield,
    stateBindToggle,
    stateBindVisibility,
} from "perry/ui"

const initialTodos = [
    "Ship Perry TODO example",
    "Verify two-way text binding",
    "Document count-driven ForEach",
]

const todoTexts = State<string[]>(initialTodos)
let doneValues = [0, 1, 0]

const input = State("")
const count = State(todoTexts.value.length)
const completedCount = State(0)
const activeCount = State(0)

function syncStats() {
    let completed = 0
    let i = 0

    while (i < doneValues.length) {
        if (doneValues[i] !== 0) {
            completed += 1
        }
        i += 1
    }

    completedCount.set(completed)
    activeCount.set(doneValues.length - completed)
}

function addTodo() {
    const text = input.value
    if (text.length === 0) {
        return
    }

    const nextTexts = [...todoTexts.value, text]
    todoTexts.set(nextTexts)
    doneValues = [...doneValues, 0]
    count.set(nextTexts.length)
    syncStats()
    input.set("")
}

function deleteTodo(index: number) {
    const nextTexts = todoTexts.value.filter((_, i) => i !== index)
    const nextDoneValues: number[] = []

    let i = 0
    while (i < doneValues.length) {
        if (i !== index) {
            nextDoneValues.push(doneValues[i])
        }
        i += 1
    }

    todoTexts.set(nextTexts)
    doneValues = nextDoneValues
    count.set(nextTexts.length)
    syncStats()
}

function clearCompleted() {
    const nextTexts: string[] = []
    const nextDoneValues: number[] = []

    let i = 0
    while (i < todoTexts.value.length) {
        if (doneValues[i] === 0) {
            nextTexts.push(todoTexts.value[i])
            nextDoneValues.push(doneValues[i])
        }
        i += 1
    }

    todoTexts.set(nextTexts)
    doneValues = nextDoneValues
    count.set(nextTexts.length)
    syncStats()
}

const field = TextField("What needs to be done?", (value: string) => input.set(value))
stateBindTextfield(input, field)

const emptyState = Text("No todos yet. Add one above.")

const todoList = ForEach(count, (i: number) => {
    const rowDone = State(doneValues[i])
    const toggle = Toggle(todoTexts.value[i], (on: boolean) => {
        doneValues[i] = on ? 1 : 0
        syncStats()
    })

    stateBindToggle(rowDone, toggle)
    rowDone.set(doneValues[i])

    return HStack(8, [
        toggle,
        Spacer(),
        Button("Delete", () => {
            deleteTodo(i)
        }),
    ])
})

stateBindVisibility(count, todoList, emptyState)
syncStats()

App({
    title: "Todo App",
    width: 560,
    height: 520,
    body: VStack(16, [
        HStack(8, [
            Text("My Todos"),
            Spacer(),
            Button("Clear Completed", () => {
                clearCompleted()
            }),
        ]),

        Text(`${count.value} total · ${completedCount.value} completed · ${activeCount.value} active`),

        HStack(8, [
            field,
            Button("Add", () => {
                addTodo()
            }),
        ]),

        Divider(),
        emptyState,
        todoList,
        Spacer(),
        Text("Toggle marks a task done. Delete removes it."),
    ]),
})
