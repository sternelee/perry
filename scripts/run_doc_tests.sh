#!/usr/bin/env bash
# Run the Perry doc-example test harness.
#
# Compiles every .ts under docs/examples/, runs it (UI examples with
# PERRY_UI_TEST_MODE=1), and verifies compile + exit status + optional
# stdout diffs. Invoked on macOS/Linux CI; Windows uses run_doc_tests.ps1.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$REPO_ROOT"

# Build perry + the harness in release mode (skipped if already built).
cargo build --release -p perry -p perry-runtime -p perry-stdlib -p perry-doc-tests

REPORT_DIR="$REPO_ROOT/docs/examples/_reports"
mkdir -p "$REPORT_DIR"

REPORT_JSON="$REPORT_DIR/latest.json"

# Forward any extra args through to the harness (e.g. --filter, --verbose).
exec cargo run --release --quiet -p perry-doc-tests -- \
    --json "$REPORT_JSON" \
    "$@"
