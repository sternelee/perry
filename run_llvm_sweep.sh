#!/usr/bin/env bash
# Perry Parity Sweep
# Compiles all test-files/test_*.ts, diffs output against Node.js,
# and reports MATCH/DIFF/CRASH/COMPILE_FAIL counts.
#
# Usage:
#   ./run_llvm_sweep.sh              # Run all tests
#   ./run_llvm_sweep.sh test_array   # Run only matching tests
#   PERRY_TIMEOUT=30 ./run_llvm_sweep.sh  # Custom timeout (default: 10s)

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PERRY="${SCRIPT_DIR}/target/release/perry"
OUT_DIR="${PERRY_SWEEP_DIR:-/tmp/llvm_sweep_out}"
TIMEOUT_SEC="${PERRY_TIMEOUT:-10}"
FILTER="${1:-}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Find timeout command
if command -v timeout &>/dev/null; then
    TIMEOUT_CMD="timeout"
elif command -v gtimeout &>/dev/null; then
    TIMEOUT_CMD="gtimeout"
else
    TIMEOUT_CMD=""
fi

run_with_timeout() {
    local secs=$1; shift
    if [[ -n "$TIMEOUT_CMD" ]]; then
        $TIMEOUT_CMD "$secs" "$@"
    else
        "$@"
    fi
}

# Ensure binary exists
if [[ ! -x "$PERRY" ]]; then
    echo "Building Perry (release)..."
    cargo build --release -p perry --quiet 2>/dev/null || {
        echo -e "${RED}Build failed${NC}"
        exit 1
    }
fi

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.diff "$OUT_DIR"/*.compile.log "$OUT_DIR"/summary.txt

# Counters
COMPILE_PASS=0
COMPILE_FAIL=0
RUN_MATCH=0
RUN_DIFF=0
RUN_CRASH=0
RUN_TIMEOUT=0
NODE_FAIL=0
TOTAL=0

# Track results for summary
declare -a MATCHES=()
declare -a DIFFS=()
declare -a CRASHES=()
declare -a COMPILE_FAILS=()

echo "========================================"
echo "   Perry LLVM Backend Sweep"
echo "========================================"
echo ""

for f in "$SCRIPT_DIR"/test-files/test_*.ts; do
    [[ -d "$f" ]] && continue
    name=$(basename "$f" .ts)

    # Optional filter
    if [[ -n "$FILTER" && "$name" != *"$FILTER"* ]]; then
        continue
    fi

    TOTAL=$((TOTAL + 1))
    bin="$OUT_DIR/$name.bin"

    # Compile (LLVM is the only backend post-cutover)
    if ! "$PERRY" compile "$f" -o "$bin" >"$OUT_DIR/$name.compile.log" 2>&1; then
        COMPILE_FAIL=$((COMPILE_FAIL + 1))
        COMPILE_FAILS+=("$name")
        echo -e "${RED}COMPILE_FAIL${NC}  $name"
        echo "$name COMPILE_FAIL" >>"$OUT_DIR/summary.txt"
        continue
    fi
    COMPILE_PASS=$((COMPILE_PASS + 1))

    # Run LLVM binary
    llvm_out=$(run_with_timeout "$TIMEOUT_SEC" "$bin" 2>&1)
    llvm_exit=$?

    if [[ $llvm_exit -eq 124 ]]; then
        RUN_TIMEOUT=$((RUN_TIMEOUT + 1))
        echo -e "${YELLOW}TIMEOUT${NC}       $name"
        echo "$name TIMEOUT" >>"$OUT_DIR/summary.txt"
        rm -f "$bin"
        continue
    fi

    # Run with Node.js (filter stderr warnings about --experimental-strip-types)
    node_out=$(run_with_timeout "$TIMEOUT_SEC" node --experimental-strip-types "$f" 2>/dev/null)
    node_exit=$?

    if [[ $node_exit -ne 0 && $node_exit -ne 124 ]]; then
        NODE_FAIL=$((NODE_FAIL + 1))
        echo -e "${YELLOW}NODE_FAIL${NC}     $name"
        echo "$name NODE_FAIL" >>"$OUT_DIR/summary.txt"
        rm -f "$bin"
        continue
    fi

    # Compare
    if [[ "$llvm_out" == "$node_out" ]]; then
        RUN_MATCH=$((RUN_MATCH + 1))
        MATCHES+=("$name")
        if [[ $llvm_exit -ne 0 ]]; then
            echo -e "${GREEN}MATCH${NC}         $name (exit=$llvm_exit)"
        else
            echo -e "${GREEN}MATCH${NC}         $name"
        fi
        echo "$name MATCH" >>"$OUT_DIR/summary.txt"
    elif [[ $llvm_exit -ne 0 ]]; then
        RUN_CRASH=$((RUN_CRASH + 1))
        CRASHES+=("$name")
        echo -e "${RED}CRASH${NC}         $name (exit=$llvm_exit)"
        echo "$name CRASH (exit=$llvm_exit)" >>"$OUT_DIR/summary.txt"
        diff <(echo "$llvm_out") <(echo "$node_out") >"$OUT_DIR/$name.diff" 2>&1
    else
        RUN_DIFF=$((RUN_DIFF + 1))
        DIFFS+=("$name")
        # Count diff lines for severity indicator
        diff_lines=$(diff <(echo "$llvm_out") <(echo "$node_out") | grep -c '^[<>]')
        echo -e "${YELLOW}DIFF${NC}          $name ($diff_lines lines differ)"
        echo "$name DIFF ($diff_lines lines)" >>"$OUT_DIR/summary.txt"
        diff <(echo "$llvm_out") <(echo "$node_out") >"$OUT_DIR/$name.diff" 2>&1
    fi

    rm -f "$bin"
done

# Summary
RUNTIME_TESTED=$((RUN_MATCH + RUN_DIFF + RUN_CRASH + RUN_TIMEOUT))
if [[ $RUNTIME_TESTED -gt 0 ]]; then
    MATCH_PCT=$(echo "scale=1; $RUN_MATCH * 100 / $RUNTIME_TESTED" | bc)
else
    MATCH_PCT="0.0"
fi

echo ""
echo "========================================"
echo "   LLVM Sweep Summary"
echo "========================================"
echo -e "Total tests:    $TOTAL"
echo -e "${GREEN}Compile pass:${NC}   $COMPILE_PASS"
echo -e "${RED}Compile fail:${NC}   $COMPILE_FAIL"
echo ""
echo -e "${GREEN}MATCH Node:${NC}     $RUN_MATCH"
echo -e "${YELLOW}DIFF Node:${NC}      $RUN_DIFF"
echo -e "${RED}CRASH:${NC}          $RUN_CRASH"
echo -e "${YELLOW}TIMEOUT:${NC}        $RUN_TIMEOUT"
echo -e "${YELLOW}Node fail:${NC}      $NODE_FAIL"
echo ""
echo -e "${CYAN}Match rate:${NC}     ${MATCH_PCT}% ($RUN_MATCH/$RUNTIME_TESTED)"
echo ""
echo "Detailed diffs:  $OUT_DIR/*.diff"
echo "Compile logs:    $OUT_DIR/*.compile.log"
echo "Full summary:    $OUT_DIR/summary.txt"
