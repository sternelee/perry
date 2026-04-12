# Polyglot Benchmark Results

Best of 3 runs, macOS ARM64 (Apple Silicon). All times in milliseconds.
Lower is better.

| Benchmark      | Perry |  Rust |   C++ |    Go | Swift |  Java |  Node |  Python |
|----------------|-------|-------|-------|-------|-------|-------|-------|---------|
| fibonacci      |   936 |   315 |   308 |   446 |   399 |   279 |   991 |   15935 |
| loop_overhead  |    12 |    95 |    96 |    96 |    95 |    97 |    53 |    2979 |
| array_write    |     2 |     6 |     2 |     8 |     2 |     6 |     8 |     392 |
| array_read     |     4 |     9 |     9 |    10 |     9 |    11 |    13 |     330 |
| math_intensive |    14 |    48 |    50 |    48 |    48 |    50 |    49 |    2212 |
| object_create  |     8 |     0 |     0 |     0 |     0 |     4 |     8 |     161 |
| nested_loops   |     8 |     8 |     8 |     9 |     8 |    10 |    17 |     470 |
| accumulate     |    25 |    98 |    96 |    96 |    96 |   100 |   592 |    4919 |
