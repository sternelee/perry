// demonstrates: Issue #185 Phase C step 4 — `style: { ... }` works on
// every Widget-returning constructor (not just Button). The codegen's
// generic table-call helper now absorbs a trailing object literal as
// an inline style and applies it via the same `apply_inline_style`
// HIR pass that Button has been using since v0.5.306.
// docs: docs/src/ui/styling.md
// platforms: macos, linux, windows
// targets: ios-simulator, tvos-simulator, watchos-simulator, web, wasm, android

import { App, VStack, Text, Toggle, Slider, ProgressView } from "perry/ui"

// Text with inline style — color, font, padding, border.
const heading = Text("Settings", {
    color: { r: 0.1, g: 0.2, b: 0.4, a: 1.0 },
    padding: { top: 8, right: 12, bottom: 8, left: 12 },
    borderRadius: 4,
})

// Toggle with inline style — corner radius + opacity.
const enableNotifications = Toggle("Notifications", (on: boolean) => {
    console.log("notif:", on)
}, {
    borderRadius: 6,
    opacity: 0.95,
    tooltip: "Toggle desktop notifications",
})

// Slider with inline style — sized + decorated.
const volume = Slider(0, 100, (v: number) => {
    console.log("volume:", v)
}, {
    borderColor: { r: 0.5, g: 0.5, b: 0.5, a: 0.6 },
    borderWidth: 1,
    borderRadius: 8,
    padding: 8,
})

// ProgressView (no event arg) with inline style.
const progress = ProgressView({
    backgroundColor: { r: 0.95, g: 0.95, b: 0.95, a: 1.0 },
    borderRadius: 4,
    opacity: 0.9,
})

App({
    title: "Cross-widget inline style",
    width: 360,
    height: 320,
    body: VStack(16, [heading, enableNotifications, volume, progress]),
})
