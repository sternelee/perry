// Benchmark: Object property writes (dynamic)
// Measures: shape-transition cache performance
// Catches: v0.5.29-30 regression (43ms→6.4ms for 10k×20)

const OBJECTS = 10000;
const FIELDS = 20;

// Warmup
for (let i = 0; i < 100; i++) {
  const obj: any = {};
  for (let j = 0; j < FIELDS; j++) {
    obj["field_" + j] = j;
  }
}

const start = Date.now();

let checksum = 0;
for (let i = 0; i < OBJECTS; i++) {
  const obj: any = {};
  for (let j = 0; j < FIELDS; j++) {
    obj["field_" + j] = i * FIELDS + j;
  }
  checksum += obj["field_0"] + obj["field_" + (FIELDS - 1)];
}

const elapsed = Date.now() - start;
console.log("object_property:" + elapsed);
console.log("checksum:" + checksum);
