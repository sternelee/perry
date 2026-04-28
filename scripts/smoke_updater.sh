#!/usr/bin/env bash
# End-to-end smoke test for perry/updater on macOS / Linux.
#
# Drives the full happy path: build two versions of a tiny TS program,
# generate a fresh Ed25519 keypair, sign v1.0.1, serve a manifest over
# HTTP, run v1.0.0, and assert that v1.0.1 boots after the install.
#
# Out of scope (each is a separate follow-up):
#   - Windows  → scripts/smoke_updater.ps1 (todo)
#   - AppImage → needs appimagetool, separate runner
#   - Crash-loop rollback → exercises a different code path; own script
#   - CI integration → updater smokes are timing-sensitive enough that
#     manual pre-release runs catch more than green CI does
#
# Usage:
#   scripts/smoke_updater.sh
#   PERRY_BIN=/path/to/perry SMOKE_PORT=18765 scripts/smoke_updater.sh
#
# Exits 0 on success, non-zero with a diagnostic on any failure step.

set -uo pipefail

PERRY_BIN="${PERRY_BIN:-$(pwd)/target/release/perry}"
PORT="${SMOKE_PORT:-18765}"

if [[ ! -x "$PERRY_BIN" ]]; then
    echo "perry binary not found at $PERRY_BIN" >&2
    echo "set PERRY_BIN or run 'cargo build --release -p perry'" >&2
    exit 2
fi

for tool in openssl python3 shasum; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "required tool not found: $tool" >&2
        exit 2
    fi
done

OS_NAME="$(uname -s)"
ARCH_NAME="$(uname -m)"
case "$OS_NAME" in
    Darwin) PLATFORM_OS="darwin" ;;
    Linux)  PLATFORM_OS="linux"  ;;
    *)      echo "unsupported OS for this smoke: $OS_NAME (use the .ps1 on Windows)" >&2; exit 2 ;;
esac
case "$ARCH_NAME" in
    arm64|aarch64) PLATFORM_ARCH="aarch64" ;;
    x86_64|amd64)  PLATFORM_ARCH="x86_64"  ;;
    *)             echo "unsupported arch: $ARCH_NAME" >&2; exit 2 ;;
esac
PLATFORM_KEY="${PLATFORM_OS}-${PLATFORM_ARCH}"

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FIXTURE="$REPO_ROOT/crates/perry-updater/tests/fixtures/smoke.ts.tpl"
if [[ ! -f "$FIXTURE" ]]; then
    echo "fixture not found at $FIXTURE" >&2
    exit 2
fi

TMP="$(mktemp -d -t perry-updater-smoke-XXXXXX)"
SERVER_PID=""
cleanup() {
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    if [[ -z "${SMOKE_KEEP:-}" ]]; then
        rm -rf "$TMP"
    else
        echo "(SMOKE_KEEP set — leaving $TMP for inspection)"
    fi
}
trap cleanup EXIT

echo "==> tmp dir: $TMP"
echo "==> platform: $PLATFORM_KEY"

# ----------------------------------------------------------------------------
# 1. Generate a throwaway Ed25519 keypair.
# ----------------------------------------------------------------------------
# DER form is the easiest to slice the raw 32 bytes from — the encoded
# public key has a 12-byte ASN.1 prefix, the trailing 32 bytes are the key.
echo "==> generating Ed25519 keypair"
openssl genpkey -algorithm ED25519 -outform DER -out "$TMP/priv.der" 2>/dev/null
openssl pkey -in "$TMP/priv.der" -inform DER -pubout -outform DER -out "$TMP/pub.der" 2>/dev/null
PUBKEY_B64=$(tail -c 32 "$TMP/pub.der" | base64 | tr -d '\n ')

# ----------------------------------------------------------------------------
# 2. Render the fixture as v1.0.0 and v1.0.1, then compile both.
# ----------------------------------------------------------------------------
echo "==> compiling test binaries"
mkdir -p "$TMP/build"
for v in 1.0.0 1.0.1; do
    sed -e "s/__VERSION__/$v/g" \
        -e "s|__PUBKEY__|$PUBKEY_B64|g" \
        "$FIXTURE" > "$TMP/build/smoke_$v.ts"
    if ! "$PERRY_BIN" compile "$TMP/build/smoke_$v.ts" -o "$TMP/build/v$v" 2> "$TMP/build/v${v}.log"; then
        echo "compile of v$v failed" >&2
        cat "$TMP/build/v${v}.log" >&2
        exit 1
    fi
done

# ----------------------------------------------------------------------------
# 3. Sign v1.0.1.
# ----------------------------------------------------------------------------
# Sig domain (per the manifest contract): pure Ed25519 over the raw 32-byte
# SHA-256 digest of the binary. -rawin tells openssl pkeyutl not to apply
# its own hashing, so the bytes we hand it are exactly what gets signed.
echo "==> signing v1.0.1"
openssl dgst -sha256 -binary "$TMP/build/v1.0.1" > "$TMP/digest.bin"
openssl pkeyutl -sign \
    -inkey "$TMP/priv.der" -keyform DER \
    -rawin -in "$TMP/digest.bin" -out "$TMP/sig.bin" 2>/dev/null
SIG_B64=$(base64 < "$TMP/sig.bin" | tr -d '\n ')
SHA256_HEX=$(shasum -a 256 "$TMP/build/v1.0.1" | awk '{print $1}')
SIZE=$(wc -c < "$TMP/build/v1.0.1" | tr -d ' ')

# ----------------------------------------------------------------------------
# 4. Build the manifest and stage v1.0.1 for serving.
# ----------------------------------------------------------------------------
mkdir -p "$TMP/server"
cp "$TMP/build/v1.0.1" "$TMP/server/v1.0.1"
cat > "$TMP/server/manifest.json" <<EOF
{
  "schemaVersion": 1,
  "version": "1.0.1",
  "pubDate": "2026-04-27T00:00:00Z",
  "notes": "smoke test fixture",
  "platforms": {
    "$PLATFORM_KEY": {
      "url": "http://127.0.0.1:$PORT/v1.0.1",
      "sha256": "$SHA256_HEX",
      "signature": "$SIG_B64",
      "size": $SIZE
    }
  }
}
EOF

# ----------------------------------------------------------------------------
# 5. Serve the manifest + binary on localhost.
# ----------------------------------------------------------------------------
echo "==> starting http server on :$PORT"
( cd "$TMP/server" && exec python3 -m http.server "$PORT" --bind 127.0.0.1 ) \
    >"$TMP/server/access.log" 2>&1 &
SERVER_PID=$!

# Wait for the server to actually accept connections — python's http.server
# binds before it's ready to serve and a fast curl can race past the bind.
for _ in $(seq 1 30); do
    if curl -fsS "http://127.0.0.1:$PORT/manifest.json" >/dev/null 2>&1; then
        break
    fi
    sleep 0.1
done
if ! curl -fsS "http://127.0.0.1:$PORT/manifest.json" >/dev/null 2>&1; then
    echo "http server didn't come up on port $PORT" >&2
    exit 1
fi

# ----------------------------------------------------------------------------
# 6. Stage v1.0.0 as the "installed" binary, plus pre-stage v1.0.1 as if
#    we had already downloaded it. The fixture's TS code intentionally does
#    NOT exercise the binary-download-into-disk path — see the file-header
#    note in tests/fixtures/smoke.ts.tpl for the underlying Perry runtime
#    gap (`response.arrayBuffer()` returns a metadata-only object). The
#    smoke still hits manifest fetch + parse, hash + signature verify,
#    atomic install, and detached relaunch, which is the security-critical
#    portion of the flow.
# ----------------------------------------------------------------------------
INSTALLED="$TMP/installed/perry-smoke-app"
mkdir -p "$TMP/installed"
cp "$TMP/build/v1.0.0" "$INSTALLED"
chmod +x "$INSTALLED"

# Pre-stage v1.0.1 where the fixture expects to find it.
cp "$TMP/build/v1.0.1" "$INSTALLED.staged"
chmod +x "$INSTALLED.staged"

MARKER="$TMP/marker.txt"
: > "$MARKER"

export SMOKE_MARKER="$MARKER"
export SMOKE_MANIFEST_URL="http://127.0.0.1:$PORT/manifest.json"

echo "==> running v1.0.0 (will install + relaunch v1.0.1)"
"$INSTALLED" >"$TMP/v1.0.0.stdout" 2>"$TMP/v1.0.0.stderr"
RC=$?
if [[ $RC -ne 0 ]]; then
    echo "v1.0.0 exited with $RC" >&2
    echo "--- stdout ---"; cat "$TMP/v1.0.0.stdout"
    echo "--- stderr ---"; cat "$TMP/v1.0.0.stderr"
    exit 1
fi

# ----------------------------------------------------------------------------
# 7. Wait for v1.0.1 to write its marker line.
# ----------------------------------------------------------------------------
# v1.0.0 finishes early (it process.exit(0)s after relaunch). The detached
# v1.0.1 child is now running independently — give it a few seconds to
# write its marker line before we declare timeout.
for _ in $(seq 1 50); do
    if grep -q '^1.0.1$' "$MARKER" 2>/dev/null; then
        break
    fi
    sleep 0.2
done

# ----------------------------------------------------------------------------
# 8. Assertions.
# ----------------------------------------------------------------------------
ok=true
if ! grep -q '^1.0.0$' "$MARKER"; then
    echo "FAIL: v1.0.0 didn't record its run in the marker" >&2
    ok=false
fi
if ! grep -q '^1.0.1$' "$MARKER"; then
    echo "FAIL: v1.0.1 never appeared in the marker (relaunch didn't take)" >&2
    ok=false
fi

# Sanity: the binary at $INSTALLED should now be v1.0.1's bytes.
INSTALLED_HASH=$(shasum -a 256 "$INSTALLED" | awk '{print $1}')
if [[ "$INSTALLED_HASH" != "$SHA256_HEX" ]]; then
    echo "FAIL: installed binary hash $INSTALLED_HASH != expected v1.0.1 $SHA256_HEX" >&2
    ok=false
fi

# Sanity: <exe>.prev should hold the old v1.0.0.
if [[ ! -f "$INSTALLED.prev" ]]; then
    echo "FAIL: $INSTALLED.prev backup missing" >&2
    ok=false
fi

if [[ "$ok" != "true" ]]; then
    echo
    echo "--- marker ---"
    cat "$MARKER" 2>/dev/null || echo "(empty)"
    echo "--- v1.0.0 stdout ---"; cat "$TMP/v1.0.0.stdout"
    echo "--- v1.0.0 stderr ---"; cat "$TMP/v1.0.0.stderr"
    exit 1
fi

echo
echo "smoke test PASSED"
echo "  marker:"
sed 's/^/    /' "$MARKER"
