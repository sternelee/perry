// demonstrates: multiple Text widgets bound to the same State
// docs: docs/src/ui/state.md
// platforms: macos, linux, windows

import { App, VStack, Text, Button, State, Spacer } from "perry/ui"

const count = State(0)

App({
    title: "State Binding",
    width: 400,
    height: 400,
    body: VStack(12, [
        Text(`Count: ${count.value}`),
        Text(`Value: ${count.value} items`),
        Button("+1", () => count.set(count.value + 1)),
        Button("-1", () => count.set(count.value - 1)),
        Spacer(),
    ]),
})
