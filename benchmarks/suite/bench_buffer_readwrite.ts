// Benchmark: Buffer byte read/write performance
// Measures: inline ldrb/strb path for Uint8Array
// Catches: v0.5.38 inline buffer access regression

const SIZE = 1000000;
const buf = new Uint8Array(SIZE);

// Fill with data
for (let i = 0; i < SIZE; i++) {
  buf[i] = i % 256;
}

// Warmup
for (let w = 0; w < 3; w++) {
  let s = 0;
  for (let i = 0; i < SIZE; i++) s += buf[i];
}

const ITERATIONS = 100;
const start = Date.now();

let checksum = 0;
for (let iter = 0; iter < ITERATIONS; iter++) {
  let sum = 0;
  for (let i = 0; i < SIZE; i++) {
    sum += buf[i];
  }
  checksum += sum;
}

const elapsed = Date.now() - start;
console.log("buffer_readwrite:" + elapsed);
console.log("checksum:" + checksum);
