// Benchmark: Heavy string operations
// Measures: string concat, split, join, indexOf performance + memory
// Catches: ASCII fast path regression (c43b5a9), string alloc regressions

const ITERATIONS = 1000;

// Build a moderately large string
let base = "";
for (let i = 0; i < 1000; i++) {
  base += "word" + i + " ";
}

// Warmup
for (let w = 0; w < 3; w++) {
  const parts = base.split(" ");
  const joined = parts.join("-");
  joined.indexOf("word500");
}

const start = Date.now();

let checksum = 0;
for (let iter = 0; iter < ITERATIONS; iter++) {
  // Split + join
  const parts = base.split(" ");
  checksum += parts.length;
  const joined = parts.join("-");
  checksum += joined.length;

  // indexOf searches
  const idx1 = joined.indexOf("word500");
  const idx2 = joined.indexOf("word999");
  const idx3 = joined.indexOf("nonexistent");
  checksum += idx1 + idx2 + idx3;

  // Slice operations
  const sliced = joined.slice(100, 500);
  checksum += sliced.length;

  // startsWith/endsWith
  checksum += joined.startsWith("word0") ? 1 : 0;
  checksum += joined.endsWith("word999 ") ? 1 : 0;
}

const elapsed = Date.now() - start;
console.log("string_heavy:" + elapsed);
console.log("checksum:" + checksum);
