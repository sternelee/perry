# Other Modules

Additional npm packages and Node.js APIs supported by Perry.

## sharp (Image Processing)

```typescript,no-test
import sharp from "sharp";

await sharp("input.jpg")
  .resize(300, 200)
  .toFile("output.png");
```

## cheerio (HTML Parsing)

```typescript,no-test
import cheerio from "cheerio";

const html = "<html><body><h1>Hello</h1><p>World</p></body></html>";
const $ = cheerio.load(html);
console.log($("h1").text()); // "Hello"
```

## nodemailer (Email)

```typescript,no-test
import nodemailer from "nodemailer";

const transporter = nodemailer.createTransport({
  host: "smtp.example.com",
  port: 587,
  auth: { user: "user", pass: "pass" },
});

await transporter.sendMail({
  from: "sender@example.com",
  to: "recipient@example.com",
  subject: "Hello from Perry",
  text: "This email was sent from a compiled TypeScript binary!",
});
```

## zlib (Compression)

```typescript,no-test
import zlib from "zlib";

const compressed = zlib.gzipSync("Hello, World!");
const decompressed = zlib.gunzipSync(compressed);
```

## cron (Job Scheduling)

```typescript,no-test
import { CronJob } from "cron";

const job = new CronJob("*/5 * * * *", () => {
  console.log("Runs every 5 minutes");
});
job.start();
```

## worker_threads

```typescript,no-test
import { Worker, parentPort, workerData } from "worker_threads";

if (parentPort) {
  // Worker thread
  const data = workerData;
  parentPort.postMessage({ result: data.value * 2 });
} else {
  // Main thread
  const worker = new Worker("./worker.ts", {
    workerData: { value: 21 },
  });
  worker.on("message", (msg) => {
    console.log(msg.result); // 42
  });
}
```

## commander (CLI Parsing)

```typescript,no-test
import { Command } from "commander";

const program = new Command();
program.name("my-cli").version("1.0.0").description("My CLI tool");

program
  .command("serve")
  .option("-p, --port <number>", "Port number")
  .option("--verbose", "Verbose output")
  .action((options) => {
    console.log(`Starting server on port ${options.port}`);
  });

program.parse(process.argv);
```

## decimal.js (Arbitrary Precision)

```typescript,no-test
import Decimal from "decimal.js";

const a = new Decimal("0.1");
const b = new Decimal("0.2");
const sum = a.plus(b); // Exactly 0.3 (no floating point errors)

sum.toFixed(2);      // "0.30"
sum.toNumber();      // 0.3
a.times(b);          // 0.02
a.div(b);            // 0.5
a.pow(10);           // 1e-10
a.sqrt();            // 0.316...
```

## lru-cache

```typescript,no-test
import LRUCache from "lru-cache";

const cache = new LRUCache(100); // max 100 entries

cache.set("key", "value");
cache.get("key");       // "value"
cache.has("key");       // true
cache.delete("key");
cache.clear();
```

## child_process

```typescript,no-test
import { spawnBackground, getProcessStatus, killProcess } from "child_process";

// Spawn a background process
const { pid, handleId } = spawnBackground("sleep", ["10"], "/tmp/log.txt");

// Check if it's still running
const status = getProcessStatus(handleId);
console.log(status.alive); // true

// Kill it
killProcess(handleId);
```

## Next Steps

- [Overview](overview.md) — All stdlib modules
- [File System](fs.md) — fs and path APIs
