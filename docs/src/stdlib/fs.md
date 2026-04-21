# File System

Perry implements Node.js file system APIs for reading, writing, and managing files.

## Reading Files

```typescript,no-test
import { readFileSync } from "fs";

const content = readFileSync("config.json", "utf-8");
console.log(content);
```

### Binary File Reading

```typescript,no-test
import { readFileBuffer } from "fs";

const buffer = readFileBuffer("image.png");
console.log(`Read ${buffer.length} bytes`);
```

`readFileBuffer` reads files as binary data (uses `fs::read()` internally, not `read_to_string()`).

## Writing Files

```typescript,no-test
import { writeFileSync } from "fs";

writeFileSync("output.txt", "Hello, World!");
writeFileSync("data.json", JSON.stringify({ key: "value" }, null, 2));
```

## File Information

```typescript,no-test
import { existsSync, statSync } from "fs";

if (existsSync("config.json")) {
  const stat = statSync("config.json");
  console.log(`Size: ${stat.size}`);
}
```

## Directory Operations

```typescript,no-test
import { mkdirSync, readdirSync, rmRecursive } from "fs";

// Create directory
mkdirSync("output");

// Read directory contents
const files = readdirSync("src");
for (const file of files) {
  console.log(file);
}

// Remove directory recursively
rmRecursive("output"); // Uses fs::remove_dir_all
```

## Path Utilities

```typescript,no-test
import { join, dirname, basename, resolve } from "path";
import { fileURLToPath } from "url";

const dir = dirname(fileURLToPath(import.meta.url));
const configPath = join(dir, "config.json");
const name = basename(configPath);        // "config.json"
const abs = resolve("relative/path");     // Absolute path
```

## Next Steps

- [HTTP & Networking](http.md)
- [Overview](overview.md) — All stdlib modules
