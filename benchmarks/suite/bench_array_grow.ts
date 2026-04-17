// Benchmark: Array growth via push (memory-focused)
// Measures: peak RSS as array grows + resize overhead
// Catches: array resize strategy regressions

const SIZE = 2000000;

// Warmup
let warmup: number[] = [];
for (let i = 0; i < 10000; i++) warmup.push(i);

const start = Date.now();

const arr: number[] = [];
for (let i = 0; i < SIZE; i++) {
  arr.push(i * 1.5);
}

let checksum = 0;
for (let i = 0; i < arr.length; i += 1000) {
  checksum += arr[i];
}

const elapsed = Date.now() - start;
console.log("array_grow:" + elapsed);
console.log("length:" + arr.length);
console.log("checksum:" + checksum);
