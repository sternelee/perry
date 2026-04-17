#!/usr/bin/env bash
# Quick benchmark — runs 5 fast benchmarks in ~15 seconds
# Reports speed ratio vs Node AND peak RSS
#
# Usage: ./benchmarks/quick.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SUITE_DIR="$SCRIPT_DIR/suite"
COMPILETS="$ROOT/target/release/perry"

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

if [[ ! -f "$COMPILETS" ]]; then
  echo "Building Perry..."
  (cd "$ROOT" && cargo build --release --quiet)
fi

BENCHMARKS="05_fibonacci.ts 06_math_intensive.ts 10_nested_loops.ts 13_factorial.ts 16_matrix_multiply.ts"
HAS_NODE=0
command -v node &>/dev/null && HAS_NODE=1

extract_time() {
  echo "$1" | grep -E "^[a-z_]+:[0-9]+" | head -1 | cut -d: -f2
}

measure() {
  local tmp_err=$(mktemp) tmp_out=$(mktemp)
  /usr/bin/time -l "$@" >"$tmp_out" 2>"$tmp_err"
  local rss=0
  local pmf
  pmf=$(grep 'peak memory footprint' "$tmp_err" 2>/dev/null | awk '{print $1}')
  if [[ -n "$pmf" && "$pmf" != "0" ]]; then
    rss=$pmf
  else
    local mrs
    mrs=$(grep 'maximum resident set size' "$tmp_err" 2>/dev/null | awk '{print $1}')
    [[ -n "$mrs" ]] && rss=$mrs
  fi
  [[ -z "$rss" ]] && rss=0
  local rss_mb=$((rss / 1024 / 1024))
  local output
  output=$(cat "$tmp_out")
  rm -f "$tmp_err" "$tmp_out"
  echo "${rss_mb}|${output}"
}

echo -e "${BOLD}${CYAN}Quick Bench (5 benchmarks)${NC}"
echo ""

# Compile
cd "$SUITE_DIR"
for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  "$COMPILETS" "$bench" -o "$name" 2>/dev/null || echo "FAIL: $bench"
done

printf "${BOLD}%-18s %8s %8s %8s %8s %8s %8s${NC}\n" \
  "Benchmark" "Perry" "Node" "Ratio" "P-RSS" "N-RSS" "MemR"
echo "───────────────────────────────────────────────────────────────────"

for bench in $BENCHMARKS; do
  name="${bench%.ts}"
  display=$(echo "$name" | sed 's/^[0-9]*_//')

  # Perry
  result=$(measure "./$name")
  p_rss=$(echo "$result" | cut -d'|' -f1)
  p_out=$(echo "$result" | cut -d'|' -f2-)
  p_ms=$(extract_time "$p_out")

  # Node
  n_ms="-"; n_rss="-"
  ratio="-"; mratio="-"
  if [[ $HAS_NODE -eq 1 ]]; then
    result=$(measure node "$bench")
    n_rss=$(echo "$result" | cut -d'|' -f1)
    n_out=$(echo "$result" | cut -d'|' -f2-)
    n_ms=$(extract_time "$n_out")

    if [[ "$p_ms" =~ ^[0-9]+$ && "$n_ms" =~ ^[0-9]+$ && "$n_ms" -gt 0 ]]; then
      ratio=$(python3 -c "print(f'{$p_ms/$n_ms:.2f}x')")
      if (( p_ms < n_ms )); then
        ratio="${GREEN}${ratio}${NC}"
      else
        ratio="${RED}${ratio}${NC}"
      fi
    fi
    if [[ "$p_rss" =~ ^[0-9]+$ && "$n_rss" =~ ^[0-9]+$ && "$n_rss" -gt 0 ]]; then
      mratio=$(python3 -c "print(f'{$p_rss/$n_rss:.2f}x')")
    fi
  fi

  printf "%-18s %7sms %7sms %8b %6sMB %6sMB %8s\n" \
    "$display" "$p_ms" "$n_ms" "$ratio" "$p_rss" "$n_rss" "$mratio"

  rm -f "$SUITE_DIR/$name"
done
echo ""
