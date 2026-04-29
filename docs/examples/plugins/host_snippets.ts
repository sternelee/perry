// demonstrates: host-side perry/plugin API used in docs/src/plugins/{overview,hooks-and-events,creating-plugins}.md
// docs: docs/src/plugins/overview.md
// platforms: macos, linux, windows
// run: false

// `run: false` because end-to-end plugin loading needs a real .dylib/.so on disk.
// Compile-link is enough to certify the host-side codegen surface; this file
// pins every receiver-less call in PERRY_PLUGIN_TABLE down so a future rename
// or drop trips a link error in CI. Verifies the closure-of-#189 wiring.

// ANCHOR: imports
import {
    loadPlugin, unloadPlugin,
    emitHook, emitEvent, invokeTool,
    setPluginConfig,
    discoverPlugins, listPlugins, listHooks, listTools,
    pluginCount, initPlugins,
} from "perry/plugin"
// ANCHOR_END: imports

// ANCHOR: init
initPlugins()
setPluginConfig("api_key", "test-key")
setPluginConfig("max_retries", "3")
// ANCHOR_END: init

// ANCHOR: load
const id = loadPlugin("./counter-plugin.dylib")
console.log(`load returned: ${id !== 0 ? "ok" : "fail"}`)
// ANCHOR_END: load

// ANCHOR: discover
const found = discoverPlugins("./plugins/")
console.log(`discovered ${found.length} plugin(s)`)
// ANCHOR_END: discover

// ANCHOR: introspect
const plugins = listPlugins()
const hooks = listHooks()
const tools = listTools()
console.log(`loaded: ${pluginCount()} plugin(s), ${hooks.length} hook(s), ${tools.length} tool(s)`)
// ANCHOR_END: introspect

// ANCHOR: emit-hook
const result = emitHook("beforeSave", { content: "hello world" })
// ANCHOR_END: emit-hook

// ANCHOR: emit-event
emitEvent("dataUpdated", { source: "host", records: 100 })
// ANCHOR_END: emit-event

// ANCHOR: invoke-tool
const greeting = invokeTool("greet", { name: "Perry" })
const formatted = invokeTool("formatCode", {
    code: "const x=1",
    language: "typescript",
})
// ANCHOR_END: invoke-tool

// ANCHOR: unload
if (id !== 0) {
    unloadPlugin(id)
}
// ANCHOR_END: unload
