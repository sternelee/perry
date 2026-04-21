# Standard Library Overview

Perry natively implements many popular npm packages and Node.js APIs. When you import a supported package, Perry compiles it to native code — no JavaScript runtime involved.

## How It Works

```typescript,no-test
import fastify from "fastify";
import mysql from "mysql2/promise";
```

Perry recognizes these imports at compile time and routes them to native Rust implementations in the `perry-stdlib` crate. The API surface matches the original npm package, so existing code often works unchanged.

## Supported Packages

### Networking & HTTP
- **fastify** — HTTP server framework
- **axios** — HTTP client
- **node-fetch** / **fetch** — HTTP fetch API
- **ws** — WebSocket client/server

### Databases
- **mysql2** — MySQL client
- **pg** — PostgreSQL client
- **better-sqlite3** — SQLite
- **mongodb** — MongoDB client
- **ioredis** / **redis** — Redis client

### Cryptography
- **bcrypt** — Password hashing
- **argon2** — Password hashing (Argon2)
- **jsonwebtoken** — JWT signing/verification
- **crypto** — Node.js crypto module
- **ethers** — Ethereum library

### Utilities
- **lodash** — Utility functions
- **dayjs** / **moment** — Date manipulation
- **uuid** — UUID generation
- **nanoid** — ID generation
- **slugify** — String slugification
- **validator** — String validation

### CLI & Data
- **commander** — CLI argument parsing
- **decimal.js** — Arbitrary precision decimals
- **bignumber.js** — Big number math
- **lru-cache** — LRU caching

### Other
- **sharp** — Image processing
- **cheerio** — HTML parsing
- **nodemailer** — Email sending
- **zlib** — Compression
- **cron** — Job scheduling
- **worker_threads** — Background workers
- **exponential-backoff** — Retry logic
- **async_hooks** — AsyncLocalStorage

### Node.js Built-ins
- **fs** — File system
- **path** — Path manipulation
- **child_process** — Process spawning
- **crypto** — Cryptographic functions

## Binary Size

Perry automatically detects which stdlib features your code uses:

| Usage | Binary Size |
|-------|-------------|
| No stdlib imports | ~300KB |
| fs + path only | ~3MB |
| Full stdlib | ~48MB |

The compiler links only the required runtime components.

## compilePackages

For npm packages not natively supported, you can compile pure TypeScript/JavaScript packages natively:

```json
{
  "perry": {
    "compilePackages": ["@noble/curves", "@noble/hashes"]
  }
}
```

See [Project Configuration](../getting-started/project-config.md) for details.

## JavaScript Runtime Fallback

For packages that can't be compiled natively (native addons, dynamic code, etc.), Perry includes a QuickJS-based JavaScript runtime as a fallback:

```typescript,no-test
import { jsEval } from "perry/jsruntime";
```

## Next Steps

- [File System](fs.md)
- [HTTP & Networking](http.md)
- [Databases](database.md)
- [Cryptography](crypto.md)
- [Utilities](utilities.md)
- [Other Modules](other.md)
