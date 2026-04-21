# Hooks & Events

Perry plugins communicate through hooks, events, and tools.

## Hook Modes

Hooks support three execution modes:

### Filter Mode (default)

Each plugin receives data and returns (possibly modified) data. The output of one plugin becomes the input of the next:

```typescript,no-test
api.registerHook("transform", (data) => {
  data.content = data.content.toUpperCase();
  return data; // Returned data goes to next plugin
});
```

### Action Mode

Plugins receive data but return value is ignored. Used for side effects:

```typescript,no-test
api.registerHook("onSave", (data) => {
  console.log(`Saved: ${data.path}`);
  // Return value ignored
});
```

### Waterfall Mode

Like filter mode, but specifically for accumulating/building up a result through the chain:

```typescript,no-test
api.registerHook("buildMenu", (items) => {
  items.push({ label: "My Plugin Action", action: () => {} });
  return items;
});
```

## Hook Priority

Lower priority numbers run first:

```typescript,no-test
api.registerHook("beforeSave", validate, 10);   // Runs first
api.registerHook("beforeSave", transform, 20);   // Runs second
api.registerHook("beforeSave", log, 100);         // Runs last
```

Default priority is 50.

## Event Bus

Plugins can communicate with each other through events:

### Emitting Events

```typescript,no-test
// From a plugin
api.emit("dataUpdated", { source: "my-plugin", records: 42 });

// From the host
import { emitEvent } from "perry/plugin";
emitEvent("dataUpdated", { source: "host", records: 100 });
```

### Listening for Events

```typescript,no-test
api.on("dataUpdated", (data) => {
  console.log(`${data.source} updated ${data.records} records`);
});
```

## Tools

Plugins register callable tools:

```typescript,no-test
// Plugin registers a tool
api.registerTool("formatCode", (args) => {
  return formatSource(args.code, args.language);
});
```

```typescript,no-test
// Host invokes the tool
import { invokeTool } from "perry/plugin";

const formatted = invokeTool("formatCode", {
  code: "const x=1",
  language: "typescript",
});
```

## Configuration

Hosts can pass configuration to plugins:

```typescript,no-test
// Host sets config
import { setConfig } from "perry/plugin";
setConfig("theme", "dark");
setConfig("maxRetries", "3");
```

```typescript,no-test
// Plugin reads config
export function activate(api: PluginAPI) {
  const theme = api.getConfig("theme");     // "dark"
  const retries = api.getConfig("maxRetries"); // "3"
}
```

## Introspection

Query loaded plugins and their registrations:

```typescript,no-test
import { listPlugins, listHooks, listTools } from "perry/plugin";

const plugins = listPlugins();  // [{ name, version, description }]
const hooks = listHooks();      // [{ name, pluginName, priority }]
const tools = listTools();      // [{ name, pluginName }]
```

## Next Steps

- [Creating Plugins](creating-plugins.md) — Build a plugin
- [Overview](overview.md) — Plugin system overview
