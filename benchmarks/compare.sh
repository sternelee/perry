#!/usr/bin/env bash
# Perry Performance Regression Detector
#
# Runs benchmarks, captures speed (wall_ms) and memory (peak RSS),
# compares against baseline.json, reports regressions.
#
# Usage:
#   ./benchmarks/compare.sh                    # Run + compare against baseline
#   ./benchmarks/compare.sh --update-baseline  # Run + update baseline.json
#   ./benchmarks/compare.sh --quick            # Run only 5 fast benchmarks

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SUITE_DIR="$SCRIPT_DIR/suite"
COMPILETS="$ROOT/target/release/perry"
BASELINE="$SCRIPT_DIR/baseline.json"

# Thresholds
SPEED_THRESHOLD=15    # >15% slower = regression
MEMORY_THRESHOLD=25   # >25% more RAM = regression

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

UPDATE_BASELINE=0
QUICK_MODE=0

for arg in "$@"; do
  case "$arg" in
    --update-baseline) UPDATE_BASELINE=1 ;;
    --quick) QUICK_MODE=1 ;;
  esac
done

if [[ ! -f "$COMPILETS" ]]; then
  echo -e "${RED}Perry not found at $COMPILETS${NC}"
  echo "Run: cargo build --release"
  exit 1
fi

# Select benchmarks
if [[ $QUICK_MODE -eq 1 ]]; then
  BENCHMARKS="02_loop_overhead.ts 05_fibonacci.ts 06_math_intensive.ts 10_nested_loops.ts 13_factorial.ts"
else
  BENCHMARKS="02_loop_overhead.ts 03_array_write.ts 04_array_read.ts 05_fibonacci.ts 06_math_intensive.ts 07_object_create.ts 08_string_concat.ts 09_method_calls.ts 10_nested_loops.ts 11_prime_sieve.ts 12_binary_trees.ts 13_factorial.ts 14_closure.ts 15_mandelbrot.ts 16_matrix_multiply.ts"
fi

# Check for node
HAS_NODE=0
command -v node &>/dev/null && HAS_NODE=1

echo -e "${BOLD}${CYAN}Perry Performance Comparison (speed + RAM)${NC}"
echo ""

# ---------------------------------------------------------------------------
# Run benchmarks and collect results
# ---------------------------------------------------------------------------
RESULTS_FILE=$(mktemp)

extract_time() {
  echo "$1" | grep -E "^[a-z_]+:[0-9]+" | head -1 | cut -d: -f2
}

measure_rss() {
  # macOS: /usr/bin/time -l reports "peak memory footprint" in bytes on stderr
  # Linux: /usr/bin/time -v reports "Maximum resident set size" in KB on stderr
  local binary="$1"
  shift
  local tmp_err=$(mktemp)
  local tmp_out=$(mktemp)

  /usr/bin/time -l "$binary" "$@" >"$tmp_out" 2>"$tmp_err"
  local exit_code=$?

  local rss_bytes=0
  # macOS newer: "peak memory footprint" in bytes
  local pmf
  pmf=$(grep 'peak memory footprint' "$tmp_err" 2>/dev/null | awk '{print $1}' || true)
  if [[ -n "$pmf" && "$pmf" != "0" ]]; then
    rss_bytes=$pmf
  else
    # macOS older / some versions: "maximum resident set size" in bytes
    local mrs
    mrs=$(grep 'maximum resident set size' "$tmp_err" 2>/dev/null | awk '{print $1}' || true)
    [[ -n "$mrs" ]] && rss_bytes=$mrs
  fi
  local rss_kb=$((rss_bytes / 1024))

  local output
  output=$(cat "$tmp_out")
  rm -f "$tmp_err" "$tmp_out"

  echo "$rss_kb|$output"
}

echo -e "${BOLD}Compiling benchmarks...${NC}"
cd "$SUITE_DIR"
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  if ! "$COMPILETS" "$bench" -o "$name" 2>/dev/null; then
    echo -e "  ${RED}FAIL${NC} $bench"
  fi
done
echo ""

echo -e "${BOLD}Running benchmarks...${NC}"
if [[ $HAS_NODE -eq 1 ]]; then
  printf "${BOLD}%-20s %10s %10s %10s %10s %10s %10s${NC}\n" \
    "Benchmark" "Perry ms" "Node ms" "Ratio" "Perry KB" "Node KB" "Mem Ratio"
else
  printf "${BOLD}%-20s %10s %10s %10s${NC}\n" "Benchmark" "Perry ms" "Perry KB" "Mem KB"
fi
echo "────────────────────────────────────────────────────────────────────────────────"

set +e  # Disable errexit for measurement loop (grep/awk may return non-zero)
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  display=$(echo "$name" | sed 's/^[0-9]*_//')

  # Run Perry with RSS measurement
  perry_ms="ERR"
  perry_rss=0
  if [[ -f "$SUITE_DIR/$name" ]]; then
    result=$(measure_rss "$SUITE_DIR/$name")
    perry_rss=$(echo "$result" | head -1 | cut -d'|' -f1)
    perry_output=$(echo "$result" | sed 's/^[0-9]*|//')
    perry_ms=$(extract_time "$perry_output")
    [[ -z "$perry_ms" ]] && perry_ms="ERR"
  fi

  # Run Node with RSS measurement
  node_ms="-"
  node_rss=0
  if [[ $HAS_NODE -eq 1 ]]; then
    result=$(measure_rss node "$SUITE_DIR/$bench")
    node_rss=$(echo "$result" | head -1 | cut -d'|' -f1)
    node_output=$(echo "$result" | sed 's/^[0-9]*|//')
    node_ms=$(extract_time "$node_output")
    [[ -z "$node_ms" ]] && node_ms="-"
  fi

  # Calculate ratios
  speed_ratio="-"
  mem_ratio="-"
  if [[ "$perry_ms" != "ERR" && "$node_ms" != "-" ]]; then
    if [[ "$node_ms" -gt 0 ]] 2>/dev/null; then
      speed_ratio=$(python3 -c "print(f'{int(\"$perry_ms\")/int(\"$node_ms\"):.2f}')" 2>/dev/null || echo "-")
    fi
  fi
  if [[ "$perry_rss" -gt 0 && "$node_rss" -gt 0 ]] 2>/dev/null; then
    mem_ratio=$(python3 -c "print(f'{int(\"$perry_rss\")/int(\"$node_rss\"):.2f}')" 2>/dev/null || echo "-")
  fi

  if [[ $HAS_NODE -eq 1 ]]; then
    printf "%-20s %10s %10s %10s %10s %10s %10s\n" \
      "$display" "${perry_ms}ms" "${node_ms}ms" "$speed_ratio" "${perry_rss}KB" "${node_rss}KB" "$mem_ratio"
  else
    printf "%-20s %10s %10s %10s\n" "$display" "${perry_ms}ms" "${perry_rss}KB" "$mem_ratio"
  fi

  # Save result for JSON
  echo "${name}|${perry_ms}|${perry_rss}|${node_ms}|${node_rss}" >> "$RESULTS_FILE"
done
set -e

echo ""

# ---------------------------------------------------------------------------
# Generate current results JSON
# ---------------------------------------------------------------------------
CURRENT_JSON=$(mktemp)
python3 - "$RESULTS_FILE" "$CURRENT_JSON" <<'PYEOF'
import json, sys
results_file, output_file = sys.argv[1], sys.argv[2]
from datetime import datetime, timezone
import subprocess

commit = subprocess.run(["git", "rev-parse", "--short", "HEAD"],
                       capture_output=True, text=True).stdout.strip()

benchmarks = {}
with open(results_file) as f:
    for line in f:
        parts = line.strip().split('|')
        if len(parts) < 5: continue
        name, perry_ms, perry_rss, node_ms, node_rss = parts
        entry = {
            "perry_ms": int(perry_ms) if perry_ms not in ("ERR", "") else None,
            "perry_rss_kb": int(perry_rss) if perry_rss else 0,
        }
        if node_ms not in ("-", ""):
            entry["node_ms"] = int(node_ms)
            entry["node_rss_kb"] = int(node_rss)
            if entry["perry_ms"] and entry["node_ms"]:
                entry["speed_ratio"] = round(entry["perry_ms"] / entry["node_ms"], 3)
            if entry["perry_rss_kb"] and entry["node_rss_kb"]:
                entry["memory_ratio"] = round(entry["perry_rss_kb"] / entry["node_rss_kb"], 3)
        benchmarks[name] = entry

result = {
    "commit": commit,
    "generated_at": datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ'),
    "benchmarks": benchmarks
}
with open(output_file, 'w') as f:
    json.dump(result, f, indent=2)
PYEOF

# ---------------------------------------------------------------------------
# Compare against baseline
# ---------------------------------------------------------------------------
if [[ -f "$BASELINE" && $UPDATE_BASELINE -eq 0 ]]; then
  echo -e "${BOLD}Comparing against baseline...${NC}"
  echo ""

  python3 - "$BASELINE" "$CURRENT_JSON" "$SPEED_THRESHOLD" "$MEMORY_THRESHOLD" <<'PYEOF'
import json, sys

baseline_file, current_file = sys.argv[1], sys.argv[2]
speed_thresh = int(sys.argv[3])
mem_thresh = int(sys.argv[4])

baseline = json.load(open(baseline_file))
current = json.load(open(current_file))

regressions = []
improvements = []

print(f"Baseline commit: {baseline.get('commit', '?')} | Current commit: {current.get('commit', '?')}")
print(f"Speed threshold: {speed_thresh}% | Memory threshold: {mem_thresh}%")
print()
print(f"{'Benchmark':<20s} {'Speed Δ':>10s} {'RAM Δ':>10s} {'Status':>12s}")
print("─" * 55)

for name, cur in current["benchmarks"].items():
    base = baseline.get("benchmarks", {}).get(name)
    if not base:
        print(f"{name:<20s} {'NEW':>10s} {'NEW':>10s} {'new':>12s}")
        continue

    # Speed comparison
    speed_status = "ok"
    speed_delta = "-"
    if cur.get("perry_ms") and base.get("perry_ms") and base["perry_ms"] > 0:
        pct = (cur["perry_ms"] - base["perry_ms"]) / base["perry_ms"] * 100
        speed_delta = f"{pct:+.1f}%"
        if pct > speed_thresh:
            speed_status = "REGRESSION"
            regressions.append(f"{name}: speed +{pct:.1f}% ({base['perry_ms']}ms → {cur['perry_ms']}ms)")
        elif pct < -speed_thresh:
            speed_status = "improved"
            improvements.append(f"{name}: speed {pct:.1f}% ({base['perry_ms']}ms → {cur['perry_ms']}ms)")

    # Memory comparison
    mem_status = "ok"
    mem_delta = "-"
    if cur.get("perry_rss_kb") and base.get("perry_rss_kb") and base["perry_rss_kb"] > 0:
        pct = (cur["perry_rss_kb"] - base["perry_rss_kb"]) / base["perry_rss_kb"] * 100
        mem_delta = f"{pct:+.1f}%"
        if pct > mem_thresh:
            mem_status = "REGRESSION"
            regressions.append(f"{name}: RAM +{pct:.1f}% ({base['perry_rss_kb']}KB → {cur['perry_rss_kb']}KB)")
        elif pct < -mem_thresh:
            mem_status = "improved"
            improvements.append(f"{name}: RAM {pct:.1f}% ({base['perry_rss_kb']}KB → {cur['perry_rss_kb']}KB)")

    status = "REGRESSION" if "REGRESSION" in (speed_status, mem_status) else \
             "improved" if "improved" in (speed_status, mem_status) else "ok"
    print(f"{name.replace('_', ' '):<20s} {speed_delta:>10s} {mem_delta:>10s} {status:>12s}")

print()
if regressions:
    print(f"⚠️  {len(regressions)} REGRESSION(S):")
    for r in regressions:
        print(f"  - {r}")
    sys.exit(1)
elif improvements:
    print(f"✅ {len(improvements)} improvement(s), no regressions")
else:
    print("✅ No significant changes")
PYEOF

elif [[ $UPDATE_BASELINE -eq 1 ]]; then
  cp "$CURRENT_JSON" "$BASELINE"
  echo -e "${GREEN}Baseline updated: $BASELINE${NC}"
  echo "Commit: $(python3 -c "import json; print(json.load(open('$BASELINE'))['commit'])")"
fi

# Cleanup
rm -f "$RESULTS_FILE" "$CURRENT_JSON"
cd "$SUITE_DIR" && rm -f 02_loop_overhead 03_array_write 04_array_read 05_fibonacci \
  06_math_intensive 07_object_create 08_string_concat 09_method_calls 10_nested_loops \
  11_prime_sieve 12_binary_trees 13_factorial 14_closure 15_mandelbrot 16_matrix_multiply 2>/dev/null
