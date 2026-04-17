// Benchmark: GC pressure — allocate many short-lived objects
// Measures: peak RSS under allocation pressure + wall time
// Catches: GC trigger threshold regressions (v0.5.25: 8.45GB→36MB)

const ITERATIONS = 500000;

// Warmup
for (let i = 0; i < 1000; i++) {
  const obj = { x: i, y: i * 2, name: "item_" + i };
}

const start = Date.now();

let checksum = 0;
for (let i = 0; i < ITERATIONS; i++) {
  const obj: any = { x: i, y: i * 2, name: "item_" + i };
  const arr = [i, i + 1, i + 2];
  checksum += obj.x + arr[0];
}

const elapsed = Date.now() - start;
console.log("gc_pressure:" + elapsed);
console.log("checksum:" + checksum);
