// Benchmark: Integer arithmetic fast path
// Measures: i32 accumulator + 2D array lookup performance
// Catches: v0.5.39-43 int-analysis regressions

const SIZE = 100;
const KERNEL: number[][] = [
  [-1, -1, -1],
  [-1,  8, -1],
  [-1, -1, -1]
];

// Build a pixel grid
const pixels = new Uint8Array(SIZE * SIZE);
for (let i = 0; i < pixels.length; i++) {
  pixels[i] = i % 256;
}

// Warmup
for (let w = 0; w < 5; w++) {
  for (let y = 1; y < SIZE - 1; y++) {
    for (let x = 1; x < SIZE - 1; x++) {
      let acc = 0;
      for (let ky = -1; ky <= 1; ky++) {
        for (let kx = -1; kx <= 1; kx++) {
          acc += pixels[(y + ky) * SIZE + (x + kx)] * KERNEL[ky + 1][kx + 1];
        }
      }
    }
  }
}

const ITERATIONS = 500;
const start = Date.now();

let checksum = 0;
for (let iter = 0; iter < ITERATIONS; iter++) {
  for (let y = 1; y < SIZE - 1; y++) {
    for (let x = 1; x < SIZE - 1; x++) {
      let acc = 0;
      for (let ky = -1; ky <= 1; ky++) {
        for (let kx = -1; kx <= 1; kx++) {
          acc += pixels[(y + ky) * SIZE + (x + kx)] * KERNEL[ky + 1][kx + 1];
        }
      }
      checksum = (checksum + acc) | 0;
    }
  }
}

const elapsed = Date.now() - start;
console.log("int_arithmetic:" + elapsed);
console.log("checksum:" + checksum);
