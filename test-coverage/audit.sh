#!/usr/bin/env bash
# Perry Test Coverage Audit
#
# Scans all #[no_mangle] pub extern "C" fn declarations in perry-runtime
# and perry-stdlib, cross-references against test files, and generates
# a coverage report.
#
# Usage:
#   ./test-coverage/audit.sh              # Print report to stdout
#   ./test-coverage/audit.sh --markdown   # Generate COVERAGE.md

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
RUNTIME_DIR="$ROOT/crates/perry-runtime/src"
STDLIB_DIR="$ROOT/crates/perry-stdlib/src"
TEST_DIR="$ROOT/test-files"
CRATES_DIR="$ROOT/crates"

MARKDOWN_MODE=0
if [[ "${1:-}" == "--markdown" ]]; then
  MARKDOWN_MODE=1
fi

# ---------------------------------------------------------------------------
# 1. Collect all #[no_mangle] pub extern "C" fn names from runtime + stdlib
# ---------------------------------------------------------------------------
collect_ffi_functions() {
  local dir="$1"
  local label="$2"
  # Match patterns like:
  #   pub extern "C" fn js_foo_bar(
  #   pub unsafe extern "C" fn js_foo_bar(
  grep -rn 'pub\s\+\(unsafe\s\+\)\?extern\s\+"C"\s\+fn\s\+' "$dir" --include="*.rs" 2>/dev/null | \
    sed -E 's/.*fn ([a-zA-Z_][a-zA-Z0-9_]*)\(.*/\1/' | \
    while read -r fn_name; do
      # Find which file it's in
      local file
      file=$(grep -rl "fn ${fn_name}(" "$dir" --include="*.rs" 2>/dev/null | head -1 | sed "s|$ROOT/||")
      echo "${label}|${fn_name}|${file:-unknown}"
    done
}

echo "Scanning FFI functions..." >&2

# Collect all functions
ALL_FUNCS=$(mktemp)
collect_ffi_functions "$RUNTIME_DIR" "runtime" >> "$ALL_FUNCS"
collect_ffi_functions "$STDLIB_DIR" "stdlib" >> "$ALL_FUNCS"

TOTAL=$(wc -l < "$ALL_FUNCS" | tr -d ' ')
echo "Found $TOTAL FFI functions" >&2

# ---------------------------------------------------------------------------
# 2. Check each function against test files and Rust #[test] blocks
# ---------------------------------------------------------------------------
COVERED=0
UNCOVERED=0
RESULTS=$(mktemp)

while IFS='|' read -r crate fn_name source_file; do
  # Check TypeScript test files
  ts_hits=""
  if grep -rl "$fn_name" "$TEST_DIR"/*.ts 2>/dev/null | head -3 | grep -q .; then
    ts_hits=$(grep -rl "$fn_name" "$TEST_DIR"/*.ts 2>/dev/null | head -3 | xargs -I{} basename {} .ts | paste -sd, -)
  fi

  # Check for the function name mentioned in any #[test] block (heuristic:
  # search for the fn name in test modules across all crates)
  rust_hits=""
  if grep -rl "$fn_name" "$CRATES_DIR" --include="*.rs" 2>/dev/null | \
     xargs grep -l '#\[test\]' 2>/dev/null | head -3 | grep -q .; then
    rust_hits="yes"
  fi

  # Also check if the function's JS name appears in test files
  # e.g., js_array_push_f64 → search for "push" in test files
  js_method=""
  if [[ "$fn_name" =~ ^js_(.+)$ ]]; then
    # Extract a plausible JS method name from the last segment
    local_name="${BASH_REMATCH[1]}"
    # Get the last meaningful segment (e.g., js_array_push_f64 → push)
    method=$(echo "$local_name" | sed -E 's/.*_([a-z]+)(_f64|_i64|_ptr|_str)?$/\1/')
    if [[ "$method" != "$local_name" && ${#method} -gt 2 ]]; then
      js_method="$method"
    fi
  fi

  if [[ -n "$ts_hits" || -n "$rust_hits" ]]; then
    status="COVERED"
    ((COVERED++))
    coverage_detail="${ts_hits:+ts:$ts_hits}${rust_hits:+ rust:yes}"
  else
    status="UNCOVERED"
    ((UNCOVERED++))
    coverage_detail=""
  fi

  echo "${status}|${crate}|${fn_name}|${source_file}|${coverage_detail}" >> "$RESULTS"
done < "$ALL_FUNCS"

# ---------------------------------------------------------------------------
# 3. Generate report
# ---------------------------------------------------------------------------

# Use python3 for report generation (avoids bash 4+ associative array requirement)
if [[ $MARKDOWN_MODE -eq 1 ]]; then
  OUTPUT="$SCRIPT_DIR/COVERAGE.md"
  python3 - "$RESULTS" "$OUTPUT" "$TOTAL" "$COVERED" "$UNCOVERED" <<'PYEOF'
import sys, collections
results_file, output_file = sys.argv[1], sys.argv[2]
total, covered, uncovered = int(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5])
from datetime import datetime, timezone

file_total = collections.Counter()
file_covered = collections.Counter()
uncovered_fns = []

with open(results_file) as f:
    for line in f:
        parts = line.strip().split('|')
        if len(parts) < 4: continue
        status, crate, fn_name, source_file = parts[0], parts[1], parts[2], parts[3]
        file_total[source_file] += 1
        if status == "COVERED":
            file_covered[source_file] += 1
        else:
            uncovered_fns.append((fn_name, source_file))

pct = f"{covered * 100 / total:.1f}" if total > 0 else "0.0"

with open(output_file, 'w') as out:
    out.write(f"# Perry FFI Test Coverage\n\n")
    out.write(f"Generated: {datetime.now(timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')}\n\n")
    out.write(f"## Summary\n\n")
    out.write(f"- **Total FFI functions:** {total}\n")
    out.write(f"- **Covered (referenced in tests):** {covered}\n")
    out.write(f"- **Uncovered:** {uncovered}\n")
    out.write(f"- **Coverage:** {pct}%\n\n")
    out.write(f"## Coverage by File\n\n")
    out.write(f"| File | Total | Covered | Coverage |\n")
    out.write(f"|------|-------|---------|----------|\n")
    for f in sorted(file_total.keys()):
        t = file_total[f]
        c = file_covered[f]
        p = int(c * 100 / t) if t > 0 else 0
        out.write(f"| `{f}` | {t} | {c} | {p}% |\n")
    out.write(f"\n## Uncovered Functions\n\n")
    for fn, sf in sorted(uncovered_fns, key=lambda x: x[1]):
        out.write(f"- `{fn}` ({sf})\n")

print(f"Coverage: {covered}/{total} ({pct}%)", file=sys.stderr)
print(f"Report written to: {output_file}", file=sys.stderr)
PYEOF

else
  python3 - "$RESULTS" "$TOTAL" "$COVERED" "$UNCOVERED" <<'PYEOF'
import sys, collections
results_file = sys.argv[1]
total, covered, uncovered = int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])

file_total = collections.Counter()
file_covered = collections.Counter()
uncovered_fns = []

with open(results_file) as f:
    for line in f:
        parts = line.strip().split('|')
        if len(parts) < 4: continue
        status, crate, fn_name, source_file = parts[0], parts[1], parts[2], parts[3]
        file_total[source_file] += 1
        if status == "COVERED":
            file_covered[source_file] += 1
        else:
            uncovered_fns.append((fn_name, source_file))

pct = f"{covered * 100 / total:.1f}" if total > 0 else "0.0"
print(f"\n=== Perry FFI Test Coverage ===")
print(f"Total: {total} | Covered: {covered} | Uncovered: {uncovered} | Coverage: {pct}%\n")
print(f"--- Coverage by File ---")
for f in sorted(file_total.keys()):
    t = file_total[f]
    c = file_covered[f]
    p = int(c * 100 / t) if t > 0 else 0
    print(f"  {f:<50s} {c:3d}/{t:3d}  ({p:2d}%)")
print(f"\n--- Uncovered Functions ({uncovered} total) ---")
for fn, sf in sorted(uncovered_fns, key=lambda x: x[1]):
    print(f"  {fn:<40s} {sf}")
PYEOF
fi

# Cleanup
rm -f "$ALL_FUNCS" "$RESULTS"
