# JSON Polyglot Benchmark Results

**Runs per cell:** 11 · **Pinning:** macOS scheduler hint (taskpolicy -t 0 -l 0 — P-core preferred via throughput/latency tiers, NOT strict affinity)
**Hardware:** Darwin 25.4.0 arm64 on MacBookPro.
**Date:** 2026-04-25.

Two workloads, each language listed twice (idiomatic / optimized flag profile).
Median wall-clock time is the headline number; p95, σ (population stddev),
min, and max are reported per cell so noise is visible. Lower is better.

## JSON validate-and-roundtrip

Per iteration: parse → stringify → discard. The unmutated parse lets
Perry's lazy tape (v0.5.204+) memcpy the original blob bytes for
stringify, which is why Perry's headline number on this workload is so
low — the lazy path can avoid materializing the parse tree entirely.
10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| c++ -O3 -flto (simdjson) | optimized | 24 | 28 | 1.2 | 23 | 28 | 8 |
| c++ -O2 (simdjson) | idiomatic | 29 | 34 | 1.7 | 28 | 34 | 8 |
| perry (gen-gc + lazy tape) | optimized | 75 | 91 | 6.9 | 69 | 91 | 85 |
| rust serde_json (LTO+1cgu) | optimized | 185 | 190 | 1.7 | 183 | 190 | 11 |
| rust serde_json | idiomatic | 198 | 204 | 2.3 | 195 | 204 | 11 |
| bun (default) | idiomatic | 259 | 342 | 26.1 | 253 | 342 | 82 |
| perry (mark-sweep, no lazy) | idiomatic | 363 | 378 | 6.3 | 356 | 378 | 102 |
| node (default) | idiomatic | 394 | 602 | 60.1 | 382 | 602 | 127 |
| kotlin -server -Xmx512m | optimized | 453 | 484 | 12.6 | 447 | 484 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 473 | 533 | 21.4 | 453 | 533 | 606 |
| node --max-old=4096 | optimized | 526 | 605 | 38.3 | 478 | 605 | 128 |
| assemblyscript+json-as (wasmtime) | idiomatic | 598 | 621 | 10.5 | 582 | 621 | 58 |
| c++ -O3 -flto (nlohmann/json) | optimized | 772 | 774 | 1.1 | 771 | 774 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 805 | 824 | 9.1 | 796 | 824 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 840 | 846 | 3.0 | 836 | 846 | 25 |
| go (encoding/json) | idiomatic | 848 | 1344 | 184.3 | 796 | 1344 | 23 |
| swift -O -wmo (Foundation) | optimized | 3709 | 3793 | 32.5 | 3686 | 3793 | 34 |
| swift -O (Foundation) | idiomatic | 3730 | 3844 | 54.3 | 3688 | 3844 | 34 |

## JSON parse-and-iterate

Per iteration: parse → sum every record's nested.x (touches every element)
→ stringify. The full-tree iteration FORCES Perry's lazy tape to
materialize, so this is the honest comparison for workloads that touch
JSON content. 10k records, ~1 MB blob, 50 iterations per run.

| Implementation | Profile | Median (ms) | p95 (ms) | σ | Min | Max | Peak RSS (MB) |
|---|---|---:|---:|---:|---:|---:|---:|
| c++ -O2 (simdjson) | idiomatic | 24 | 27 | 0.9 | 24 | 27 | 8 |
| c++ -O3 -flto (simdjson) | optimized | 24 | 24 | 0.4 | 23 | 24 | 8 |
| rust serde_json (LTO+1cgu) | optimized | 183 | 185 | 1.2 | 182 | 185 | 11 |
| rust serde_json | idiomatic | 200 | 330 | 37.4 | 196 | 330 | 13 |
| bun (default) | idiomatic | 254 | 255 | 1.9 | 249 | 255 | 87 |
| node --max-old=4096 | optimized | 355 | 389 | 11.4 | 346 | 389 | 87 |
| perry (mark-sweep, no lazy) | idiomatic | 375 | 402 | 10.2 | 370 | 402 | 102 |
| node (default) | idiomatic | 380 | 652 | 87.2 | 356 | 652 | 101 |
| kotlin -server -Xmx512m | optimized | 455 | 465 | 6.1 | 444 | 465 | 426 |
| perry (gen-gc + lazy tape) | optimized | 466 | 475 | 7.0 | 457 | 475 | 100 |
| kotlin (kotlinx.serialization) | idiomatic | 469 | 481 | 5.9 | 459 | 481 | 608 |
| assemblyscript+json-as (wasmtime) | idiomatic | 605 | 632 | 11.4 | 587 | 632 | 58 |
| c++ -O3 -flto (nlohmann/json) | optimized | 786 | 793 | 2.7 | 782 | 793 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 805 | 833 | 9.2 | 798 | 833 | 22 |
| go (encoding/json) | idiomatic | 811 | 886 | 25.3 | 803 | 886 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 866 | 929 | 18.7 | 857 | 929 | 26 |
| swift -O (Foundation) | idiomatic | 3686 | 4009 | 96.7 | 3634 | 4009 | 34 |
| swift -O -wmo (Foundation) | optimized | 3702 | 3769 | 36.2 | 3660 | 3769 | 34 |
