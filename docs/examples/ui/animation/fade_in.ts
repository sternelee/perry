// demonstrates: auto-reactive animateOpacity driven by a State toggle
// docs: docs/src/ui/animation.md
// platforms: macos, linux, windows

import { App, Text, Button, VStack, State } from "perry/ui"

const visible = State(false)

const label = Text("Hello!")
label.animateOpacity(visible.value ? 1.0 : 0.0, 0.3)

App({
    title: "Animation Demo",
    width: 400,
    height: 300,
    body: VStack(16, [
        Button("Toggle", () => {
            visible.set(!visible.value)
        }),
        label,
    ]),
})
