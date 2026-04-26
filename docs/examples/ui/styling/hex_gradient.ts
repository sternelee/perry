// demonstrates: Issue #185 Phase C step 6 — compile-time string color
// parsing (hex + named) and gradient destructure. Hex strings, named
// colors, and 2-stop gradients all work as inline style values.
// docs: docs/src/ui/styling.md
// platforms: macos, linux, windows
// targets: ios-simulator, tvos-simulator, watchos-simulator, web, wasm, android

import { App, VStack, Button, Text } from "perry/ui"

// Hex string color — parsed at compile time, no runtime cost.
const heading = Text("Welcome", {
    color: "#3B82F6",
    backgroundColor: "#F3F4F6",
    borderRadius: 6,
    padding: 12,
})

// Named color — common ones work without object boilerplate.
const subheading = Text("Get started below", {
    color: "gray",
    padding: 8,
})

// Hex with alpha (#RRGGBBAA) + 3-letter shorthand.
const card = Button("Save", () => {}, {
    backgroundColor: "#3B82F6FF",
    borderColor: "#0001",        // 3-letter alpha shorthand
    borderWidth: 1,
    borderRadius: 8,
    padding: 12,
    color: "white",
})

// Gradient with 2 hex stops + an angle.
const banner = Button("Premium", () => {}, {
    gradient: {
        angle: 135,
        stops: ["#3B82F6", "#8B5CF6"],
    },
    color: "white",
    borderRadius: 12,
    padding: 16,
})

App({
    title: "Hex + gradient inline style",
    width: 400,
    height: 360,
    body: VStack(16, [heading, subheading, card, banner]),
})
