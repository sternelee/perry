# JSON Polyglot Benchmark Results

**Workload:** parse + stringify a 10,000-record (~1 MB) JSON array, 50 iterations, best-of-5.
**Hardware:** Darwin 25.4.0 arm64 on MacBookPro.
**Date:** 2026-04-25.

Each language listed twice — *idiomatic* (default release-mode flags most projects use) and *optimized* (aggressive tuning). Lower is better; sorted by time.

| Implementation | Profile | Time (ms) | Peak RSS (MB) |
|---|---|---:|---:|
| perry (gen-gc + lazy tape) | optimized | 67 | 85 |
| rust serde_json (LTO+1cgu) | optimized | 183 | 11 |
| rust serde_json | idiomatic | 193 | 11 |
| bun (default) | idiomatic | 240 | 81 |
| perry (mark-sweep, no lazy) | idiomatic | 341 | 102 |
| node (default) | idiomatic | 361 | 180 |
| node --max-old=4096 | optimized | 364 | 182 |
| kotlin -server -Xmx512m | optimized | 446 | 423 |
| kotlin (kotlinx.serialization) | idiomatic | 460 | 606 |
| c++ -O3 -flto (nlohmann/json) | optimized | 774 | 25 |
| go -ldflags="-s -w" -trimpath | optimized | 783 | 22 |
| go (encoding/json) | idiomatic | 785 | 23 |
| c++ -O2 (nlohmann/json) | idiomatic | 840 | 25 |
| swift -O -wmo (Foundation) | optimized | 3665 | 34 |
| swift -O (Foundation) | idiomatic | 3674 | 33 |
