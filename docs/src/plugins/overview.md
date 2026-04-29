# Plugin System Overview

> **Status: wired.** Both receiver-less calls (`loadPlugin`, `listPlugins`, `emitHook`, `invokeTool`, ...) and `PluginApi` instance methods (`api.registerHook`, `api.registerTool`, ...) dispatch through `crates/perry-codegen/src/lower_call.rs::PERRY_PLUGIN_TABLE` and `PERRY_PLUGIN_INSTANCE_TABLE` to the runtime FFI in `crates/perry-runtime/src/plugin.rs`. TypeScript surface lives in `types/perry/plugin/index.d.ts`. The snippets below are still kept as `text` fences because the doc-tests harness doesn't yet load a stub plugin end-to-end, but a real project that builds a plugin shared library and references it from a host program will compile, link, and run. Closed via [#189](https://github.com/PerryTS/perry/issues/189).

Perry supports native plugins as shared libraries (`.dylib`/`.so`). Plugins extend Perry applications with custom hooks, tools, services, and routes.

## How It Works

1. A plugin is a Perry-compiled shared library with `activate(api)` and `deactivate()` entry points
2. The host application loads plugins with `loadPlugin(path)`
3. Plugins register hooks, tools, and services via the API handle
4. The host dispatches events to plugins via `emitHook(name, data)`

```
Host Application
    ↓ loadPlugin("./my-plugin.dylib")
    ↓ calls plugin_activate(api_handle)
Plugin
    ↓ api.registerHook("beforeSave", callback)
    ↓ api.registerTool("format", callback)
Host
    ↓ emitHook("beforeSave", data) → plugin callback runs
```

## Quick Example

### Plugin (compiled with `--output-type dylib`)

```text
// my-plugin.ts
export function activate(api: PluginAPI) {
  api.setMetadata("my-plugin", "1.0.0", "A sample plugin");

  api.registerHook("beforeSave", (data) => {
    console.log("About to save:", data);
    return data; // Return modified data (filter mode)
  });

  api.registerTool("greet", (args) => {
    return `Hello, ${args.name}!`;
  });
}

export function deactivate() {
  console.log("Plugin deactivated");
}
```

```bash
perry my-plugin.ts --output-type dylib -o my-plugin.dylib
```

### Host Application

```text
import { loadPlugin, emitHook, invokeTool, listPlugins } from "perry/plugin";

loadPlugin("./my-plugin.dylib");

// List loaded plugins
const plugins = listPlugins();
console.log(plugins); // [{ name: "my-plugin", version: "1.0.0", ... }]

// Emit a hook
const result = emitHook("beforeSave", { content: "..." });

// Invoke a tool
const greeting = invokeTool("greet", { name: "Perry" });
console.log(greeting); // "Hello, Perry!"
```

## Plugin ABI

Plugins must export these symbols:
- `perry_plugin_abi_version()` — Returns ABI version (for compatibility checking)
- `plugin_activate(api_handle)` — Called when plugin is loaded
- `plugin_deactivate()` — Called when plugin is unloaded

Perry generates these automatically from your `activate`/`deactivate` exports.

## Native Extensions

Perry also supports **native extensions** — packages that bundle platform-specific Rust/Swift/JNI code and compile directly into your binary. These are used for accessing platform APIs like the App Store review prompt or StoreKit in-app purchases.

See [Native Extensions](native-extensions.md) for details.

## Next Steps

- [Creating Plugins](creating-plugins.md) — Build a plugin step by step
- [Hooks & Events](hooks-and-events.md) — Hook modes, event bus, tools
- [Native Extensions](native-extensions.md) — Extensions with platform-native code
- [App Store Review](appstore-review.md) — Native review prompt (iOS/Android)
