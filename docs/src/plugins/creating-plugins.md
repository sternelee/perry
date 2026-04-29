# Creating Plugins

> **Status: wired** ([#189](https://github.com/PerryTS/perry/issues/189) closed). See [Plugin System Overview — Status](overview.md) for the full surface. Snippets below are still kept as `text` fences because the doc-tests harness doesn't yet build + load a stub plugin end-to-end; a real project with a plugin shared library will compile and link.

Build Perry plugins as shared libraries that extend host applications.

## Step 1: Write the Plugin

```text
// counter-plugin.ts
let count = 0;

export function activate(api: PluginAPI) {
  api.setMetadata("counter", "1.0.0", "Counts hook invocations");

  api.registerHook("onRequest", (data) => {
    count++;
    console.log(`Request #${count}`);
    return data;
  });

  api.registerTool("getCount", () => {
    return count;
  });
}

export function deactivate() {
  console.log(`Total requests processed: ${count}`);
}
```

## Step 2: Compile as Shared Library

```bash
perry counter-plugin.ts --output-type dylib -o counter-plugin.dylib
```

The `--output-type dylib` flag tells Perry to produce a `.dylib` (macOS) or `.so` (Linux) instead of an executable.

Perry automatically:
- Generates `perry_plugin_abi_version()` returning the current ABI version
- Generates `plugin_activate(api_handle)` calling your `activate()` function
- Generates `plugin_deactivate()` calling your `deactivate()` function
- Exports symbols with `-rdynamic` for the host to find

## Step 3: Load from Host

```text
// host-app.ts
import { loadPlugin, emitHook, invokeTool, discoverPlugins } from "perry/plugin";

// Load a specific plugin
loadPlugin("./counter-plugin.dylib");

// Or discover plugins in a directory
discoverPlugins("./plugins/");

// Use the plugin
emitHook("onRequest", { path: "/api/users" });
const count = invokeTool("getCount", {});
console.log(`Processed ${count} requests`);
```

## Plugin API Reference

The `api` object passed to `activate()` provides:

### Metadata

```text
api.setMetadata(name: string, version: string, description: string)
```

### Hooks

```text
api.registerHook(name: string, callback: (data: any) => any, priority?: number)
```

Hooks are called in priority order (lower number = called first).

### Tools

```text
api.registerTool(name: string, callback: (args: any) => any)
```

Tools are invoked by name from the host.

### Configuration

```text
const value = api.getConfig(key: string)  // Read host-provided config
```

### Events

```text
api.on(event: string, handler: (data: any) => void)  // Listen for events
api.emit(event: string, data: any)                     // Emit to other plugins
```

## Next Steps

- [Hooks & Events](hooks-and-events.md) — Hook modes, event bus
- [Overview](overview.md) — Plugin system overview
