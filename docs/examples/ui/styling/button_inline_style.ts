// demonstrates: Issue #185 Phase C step 2 — inline style: { ... } object
// on the Button constructor. Codegen destructures the trailing arg into
// a sequence of setter calls at HIR time. Mirrors React-style ergonomics
// while compiling to the same FFI as the verbose imperative pattern.
// docs: docs/src/ui/styling.md
// platforms: macos, linux, windows
// targets: ios-simulator, tvos-simulator, watchos-simulator, web, wasm, android

import { App, VStack, Button } from "perry/ui"

// Single-value scalar props supported in step 2: borderRadius, opacity,
// borderWidth, tooltip, hidden, enabled. Colors / padding / shadow /
// gradient land in step 3 (multi-arg destructure).
const card = Button("Save", () => {
    console.log("saved")
}, {
    borderRadius: 8,
    borderWidth: 1,
    opacity: 0.95,
    tooltip: "Save the current document",
    enabled: true,
})

App({
    title: "Inline Style Demo",
    width: 320,
    height: 240,
    body: VStack(16, [card]),
})
