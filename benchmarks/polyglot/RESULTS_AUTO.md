# Polyglot Compute-Microbench Results (auto-generated)

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.4.0 arm64 on MacBookPro · **Date:** 2026-04-25
**Perry version:** v0.5.249

Headline = median wall-clock ms. Lower is better.

| Benchmark           | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |   Bun | Hermes |  Python |
|---------------------|-------|-------|-------|-------|-------|-------|-------|-------|--------|---------|
| fibonacci           |   318 |   330 |   315 |   451 |   406 |   282 |  1022 |   589 |      - |   16054 |
| loop_overhead       |    12 |    98 |    98 |    98 |   143 |   100 |    54 |    46 |      - |    3019 |
| loop_data_dependent |   235 |   229 |   129 |   128 |   233 |   229 |   322 |   232 |      - |   10750 |
| array_write         |     4 |     7 |     3 |     9 |     2 |     7 |     9 |     6 |      - |     401 |
| array_read          |     4 |     9 |     9 |    11 |     9 |    12 |    13 |    16 |      - |     342 |
| math_intensive      |    14 |    48 |    51 |    49 |    50 |    74 |    51 |    51 |      - |    2238 |
| object_create       |     1 |     0 |     0 |     0 |     0 |     5 |    11 |     6 |      - |     164 |
| nested_loops        |    18 |     8 |     8 |    10 |     8 |    11 |    18 |    21 |      - |     484 |
| accumulate          |    34 |    98 |    98 |    98 |    98 |   100 |   617 |   100 |      - |    5048 |

## Per-cell full stats

Format: median (p95: X, σ: S, min: Y, max: Z) ms

| Benchmark | Runtime | Stats (ms) |
|---|---|---|
| fibonacci | perry | 318 (p95: 358, σ: 13.4, min: 315, max: 358) |
| fibonacci | rust | 330 (p95: 662, σ: 115.5, min: 322, max: 662) |
| fibonacci | cpp | 315 (p95: 317, σ: 1.1, min: 314, max: 317) |
| fibonacci | go | 451 (p95: 462, σ: 4.1, min: 448, max: 462) |
| fibonacci | swift | 406 (p95: 409, σ: 0.9, min: 406, max: 409) |
| fibonacci | java | 282 (p95: 364, σ: 23.9, min: 279, max: 364) |
| fibonacci | node | 1022 (p95: 1656, σ: 182.3, min: 1008, max: 1656) |
| fibonacci | bun | 589 (p95: 1442, σ: 250.3, min: 537, max: 1442) |
| fibonacci | hermes | - |
| fibonacci | python | 16054 (p95: 20114, σ: 1178.7, min: 15874, max: 20114) |
| loop_overhead | perry | 12 (p95: 13, σ: 0.5, min: 12, max: 13) |
| loop_overhead | rust | 98 (p95: 264, σ: 61.0, min: 97, max: 264) |
| loop_overhead | cpp | 98 (p95: 197, σ: 33.7, min: 98, max: 197) |
| loop_overhead | go | 98 (p95: 99, σ: 0.6, min: 97, max: 99) |
| loop_overhead | swift | 143 (p95: 282, σ: 49.4, min: 97, max: 282) |
| loop_overhead | java | 100 (p95: 126, σ: 7.6, min: 98, max: 126) |
| loop_overhead | node | 54 (p95: 58, σ: 1.2, min: 54, max: 58) |
| loop_overhead | bun | 46 (p95: 63, σ: 6.2, min: 43, max: 63) |
| loop_overhead | hermes | - |
| loop_overhead | python | 3019 (p95: 5066, σ: 587.6, min: 2979, max: 5066) |
| loop_data_dependent | perry | 235 (p95: 307, σ: 29.3, min: 229, max: 307) |
| loop_data_dependent | rust | 229 (p95: 247, σ: 7.5, min: 227, max: 247) |
| loop_data_dependent | cpp | 129 (p95: 130, σ: 0.9, min: 128, max: 130) |
| loop_data_dependent | go | 128 (p95: 130, σ: 1.0, min: 127, max: 130) |
| loop_data_dependent | swift | 233 (p95: 278, σ: 18.4, min: 229, max: 278) |
| loop_data_dependent | java | 229 (p95: 231, σ: 0.8, min: 229, max: 231) |
| loop_data_dependent | node | 322 (p95: 447, σ: 63.4, min: 259, max: 447) |
| loop_data_dependent | bun | 232 (p95: 241, σ: 3.9, min: 230, max: 241) |
| loop_data_dependent | hermes | - |
| loop_data_dependent | python | 10750 (p95: 35545, σ: 8839.0, min: 8201, max: 35545) |
| array_write | perry | 4 (p95: 5, σ: 0.6, min: 3, max: 5) |
| array_write | rust | 7 (p95: 8, σ: 0.4, min: 7, max: 8) |
| array_write | cpp | 3 (p95: 4, σ: 0.7, min: 2, max: 4) |
| array_write | go | 9 (p95: 10, σ: 0.6, min: 8, max: 10) |
| array_write | swift | 2 (p95: 3, σ: 0.4, min: 2, max: 3) |
| array_write | java | 7 (p95: 7, σ: 0.4, min: 6, max: 7) |
| array_write | node | 9 (p95: 10, σ: 0.7, min: 8, max: 10) |
| array_write | bun | 6 (p95: 9, σ: 1.0, min: 5, max: 9) |
| array_write | hermes | - |
| array_write | python | 401 (p95: 431, σ: 10.2, min: 396, max: 431) |
| array_read | perry | 4 (p95: 5, σ: 0.9, min: 2, max: 5) |
| array_read | rust | 9 (p95: 10, σ: 0.4, min: 9, max: 10) |
| array_read | cpp | 9 (p95: 10, σ: 0.4, min: 9, max: 10) |
| array_read | go | 11 (p95: 12, σ: 0.5, min: 10, max: 12) |
| array_read | swift | 9 (p95: 11, σ: 0.6, min: 9, max: 11) |
| array_read | java | 12 (p95: 23, σ: 4.0, min: 11, max: 23) |
| array_read | node | 13 (p95: 18, σ: 1.4, min: 13, max: 18) |
| array_read | bun | 16 (p95: 19, σ: 1.2, min: 14, max: 19) |
| array_read | hermes | - |
| array_read | python | 342 (p95: 356, σ: 5.6, min: 332, max: 356) |
| math_intensive | perry | 14 (p95: 15, σ: 0.5, min: 14, max: 15) |
| math_intensive | rust | 48 (p95: 49, σ: 0.6, min: 47, max: 49) |
| math_intensive | cpp | 51 (p95: 51, σ: 0.4, min: 50, max: 51) |
| math_intensive | go | 49 (p95: 50, σ: 0.4, min: 49, max: 50) |
| math_intensive | swift | 50 (p95: 88, σ: 11.0, min: 49, max: 88) |
| math_intensive | java | 74 (p95: 135, σ: 30.3, min: 51, max: 135) |
| math_intensive | node | 51 (p95: 52, σ: 0.7, min: 50, max: 52) |
| math_intensive | bun | 51 (p95: 52, σ: 0.5, min: 51, max: 52) |
| math_intensive | hermes | - |
| math_intensive | python | 2238 (p95: 2347, σ: 35.3, min: 2227, max: 2347) |
| object_create | perry | 1 (p95: 1, σ: 0.5, min: 0, max: 1) |
| object_create | rust | 0 (p95: 1, σ: 0.3, min: 0, max: 1) |
| object_create | cpp | 0 (p95: 1, σ: 0.4, min: 0, max: 1) |
| object_create | go | 0 (p95: 1, σ: 0.3, min: 0, max: 1) |
| object_create | swift | 0 (p95: 1, σ: 0.3, min: 0, max: 1) |
| object_create | java | 5 (p95: 6, σ: 0.6, min: 4, max: 6) |
| object_create | node | 11 (p95: 20, σ: 3.4, min: 8, max: 20) |
| object_create | bun | 6 (p95: 7, σ: 0.5, min: 6, max: 7) |
| object_create | hermes | - |
| object_create | python | 164 (p95: 224, σ: 17.5, min: 160, max: 224) |
| nested_loops | perry | 18 (p95: 19, σ: 0.7, min: 17, max: 19) |
| nested_loops | rust | 8 (p95: 9, σ: 0.3, min: 8, max: 9) |
| nested_loops | cpp | 8 (p95: 8, σ: 0.0, min: 8, max: 8) |
| nested_loops | go | 10 (p95: 15, σ: 1.6, min: 9, max: 15) |
| nested_loops | swift | 8 (p95: 31, σ: 7.1, min: 8, max: 31) |
| nested_loops | java | 11 (p95: 11, σ: 0.5, min: 10, max: 11) |
| nested_loops | node | 18 (p95: 25, σ: 2.2, min: 17, max: 25) |
| nested_loops | bun | 21 (p95: 24, σ: 1.1, min: 20, max: 24) |
| nested_loops | hermes | - |
| nested_loops | python | 484 (p95: 717, σ: 66.9, min: 472, max: 717) |
| accumulate | perry | 34 (p95: 36, σ: 0.8, min: 33, max: 36) |
| accumulate | rust | 98 (p95: 99, σ: 0.8, min: 96, max: 99) |
| accumulate | cpp | 98 (p95: 98, σ: 0.5, min: 97, max: 98) |
| accumulate | go | 98 (p95: 101, σ: 1.0, min: 97, max: 101) |
| accumulate | swift | 98 (p95: 229, σ: 37.7, min: 97, max: 229) |
| accumulate | java | 100 (p95: 101, σ: 0.9, min: 98, max: 101) |
| accumulate | node | 617 (p95: 745, σ: 37.6, min: 610, max: 745) |
| accumulate | bun | 100 (p95: 101, σ: 0.7, min: 99, max: 101) |
| accumulate | hermes | - |
| accumulate | python | 5048 (p95: 5949, σ: 335.7, min: 4971, max: 5949) |
