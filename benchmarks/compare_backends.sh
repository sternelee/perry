#!/usr/bin/env bash
# Compare Cranelift vs LLVM backend: compile time, binary size, runtime perf
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PERRY="$PROJECT_DIR/target/release/perry"
OUT_DIR=$(mktemp -d)
trap "rm -rf $OUT_DIR" EXIT

BENCHMARKS="${*:-bench_fibonacci bench_array_ops bench_string_ops}"
BACKENDS="cranelift llvm"

echo "========================================"
echo "  Perry Backend Comparison"
echo "  Cranelift vs LLVM"
echo "========================================"
echo ""

# Ensure compiler is built
echo "Building compiler..."
cd "$PROJECT_DIR"
cargo build --release -p perry -p perry-runtime -p perry-stdlib 2>&1 | tail -3
echo ""

cd "$SCRIPT_DIR"

# --- Compile time & binary size ---
printf "%-25s %15s %15s %15s %15s\n" "Benchmark" "CL compile(ms)" "LLVM compile(ms)" "CL size(KB)" "LLVM size(KB)"
printf "%-25s %15s %15s %15s %15s\n" "--------" "---------" "---------" "--------" "---------"

for bench in $BENCHMARKS; do
  ts_file="${bench}.ts"
  if [ ! -f "$ts_file" ]; then
    echo "  SKIP $bench (no $ts_file)"
    continue
  fi

  cl_bin="$OUT_DIR/${bench}_cranelift"
  ll_bin="$OUT_DIR/${bench}_llvm"

  # Cranelift compile
  cl_start=$(perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000')
  "$PERRY" compile "$ts_file" -o "$cl_bin" >/dev/null 2>&1 || true
  cl_end=$(perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000')
  cl_ms=$((cl_end - cl_start))

  # LLVM compile
  ll_start=$(perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000')
  "$PERRY" compile "$ts_file" -o "$ll_bin" >/dev/null 2>&1 || true
  ll_end=$(perl -MTime::HiRes=time -e 'printf "%.0f\n", time()*1000')
  ll_ms=$((ll_end - ll_start))

  # Binary sizes
  cl_size="N/A"
  ll_size="N/A"
  if [ -f "$cl_bin" ]; then
    cl_size=$(( $(stat -f%z "$cl_bin" 2>/dev/null || stat -c%s "$cl_bin" 2>/dev/null) / 1024 ))
  fi
  if [ -f "$ll_bin" ]; then
    ll_size=$(( $(stat -f%z "$ll_bin" 2>/dev/null || stat -c%s "$ll_bin" 2>/dev/null) / 1024 ))
  fi

  printf "%-25s %15s %15s %15s %15s\n" "$bench" "${cl_ms}ms" "${ll_ms}ms" "${cl_size}KB" "${ll_size}KB"
done

echo ""
echo "--- Runtime Performance ---"
printf "%-25s %15s %15s %12s\n" "Benchmark" "Cranelift(ms)" "LLVM(ms)" "Speedup"
printf "%-25s %15s %15s %12s\n" "--------" "---------" "---------" "-------"

for bench in $BENCHMARKS; do
  cl_bin="$OUT_DIR/${bench}_cranelift"
  ll_bin="$OUT_DIR/${bench}_llvm"

  cl_total="N/A"
  ll_total="N/A"

  if [ -f "$cl_bin" ]; then
    cl_out=$("$cl_bin" 2>/dev/null) || true
    cl_total=$(echo "$cl_out" | grep "^TOTAL:" | cut -d: -f2)
    [ -z "$cl_total" ] && cl_total="N/A"
  fi

  if [ -f "$ll_bin" ]; then
    ll_out=$("$ll_bin" 2>/dev/null) || true
    ll_total=$(echo "$ll_out" | grep "^TOTAL:" | cut -d: -f2)
    [ -z "$ll_total" ] && ll_total="N/A"
  fi

  speedup="N/A"
  if [ "$cl_total" != "N/A" ] && [ "$ll_total" != "N/A" ] && [ "$ll_total" -gt 0 ] 2>/dev/null; then
    speedup=$(echo "scale=2; $cl_total / $ll_total" | bc 2>/dev/null || echo "N/A")
    speedup="${speedup}x"
  fi

  printf "%-25s %15s %15s %12s\n" "$bench" "${cl_total}ms" "${ll_total}ms" "$speedup"
done

echo ""
echo "--- Node.js Reference ---"
printf "%-25s %15s\n" "Benchmark" "Node.js(ms)"
printf "%-25s %15s\n" "--------" "---------"

for bench in $BENCHMARKS; do
  ts_file="${bench}.ts"
  if [ ! -f "$ts_file" ]; then continue; fi
  node_out=$(node --experimental-strip-types "$ts_file" 2>/dev/null) || true
  node_total=$(echo "$node_out" | grep "^TOTAL:" | cut -d: -f2)
  [ -z "$node_total" ] && node_total="N/A"
  printf "%-25s %15s\n" "$bench" "${node_total}ms"
done

echo ""
echo "Done."
