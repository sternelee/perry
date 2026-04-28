// Type declarations for perry/updater — Perry's auto-update primitives.
//
// This module exposes the security-critical and platform-touching pieces
// (semver compare, hash + Ed25519 signature verification, atomic install,
// sentinel-based rollback, detached relaunch). The download itself is left
// to the TS layer using the existing `fetch()` API — see `@perry/updater`
// for the high-level Tauri-style wrapper around these primitives.

/**
 * Compare two semver versions.
 *
 * Returns -1 if `current < candidate` (an update is available),
 *         0 if equal,
 *         1 if `current > candidate`,
 *        -2 if either input fails to parse.
 */
export function compareVersions(current: string, candidate: string): number;

/**
 * Verify the SHA-256 digest of a file matches an expected lowercase hex string.
 * Returns 1 on match, 0 on any failure (file missing, mismatch, unreadable).
 */
export function verifyHash(filePath: string, expectedHex: string): number;

/**
 * Compute the SHA-256 hex digest of a file. Returns empty string on failure.
 * Useful for logging the actual hash on a `verifyHash` mismatch.
 */
export function computeFileSha256(filePath: string): string;

/**
 * Verify an Ed25519 signature over the SHA-256 digest of a file.
 *
 * The signed payload is the **raw 32-byte SHA-256 digest** of the file
 * (NOT the hex string, NOT the file bytes themselves). Sign side must
 * compute SHA-256 → sign the raw 32 bytes with the Ed25519 secret key.
 *
 * @param sigB64    base64-encoded 64-byte signature
 * @param pubkeyB64 base64-encoded 32-byte public key
 * Returns 1 on valid signature, 0 on any error (size, decode, mismatch).
 */
export function verifySignature(
  filePath: string,
  sigB64: string,
  pubkeyB64: string,
): number;

/**
 * Atomically write `payload` to `sentinelPath`, creating the parent directory
 * if needed. Returns 1 on success, 0 on any IO error.
 */
export function writeSentinel(sentinelPath: string, payload: string): number;

/**
 * Read the sentinel file. Returns the contents as a string, or empty string
 * if the file is missing/unreadable. The caller (TS) parses the JSON.
 */
export function readSentinel(sentinelPath: string): string;

/**
 * Delete the sentinel file. Returns 1 on success or if the file did not
 * exist to begin with (idempotent), 0 on any other IO error.
 */
export function clearSentinel(sentinelPath: string): number;

// --- Desktop platform helpers (the `desktop` module of perry-updater) ---

/**
 * Resolve the path to the running executable, accounting for platform quirks:
 * - macOS: returns the surrounding `.app` bundle path if applicable.
 * - Linux: honors `$APPIMAGE` when set (the AppImage runtime points
 *   `current_exe()` inside a read-only squashfs mount).
 * - Windows / bare ELF / bare Mach-O: returns the canonical exe path.
 */
export function getExePath(): string;

/**
 * Sibling backup path: `<exe>.prev`. This is where `installUpdate` keeps the
 * previous version so `performRollback` can restore it.
 */
export function getBackupPath(): string;

/**
 * Per-OS user-writable sentinel path:
 * - macOS:   `~/Library/Application Support/<app>/updater.sentinel`
 * - Windows: `%LOCALAPPDATA%\<app>\updater.sentinel`
 * - Linux:   `$XDG_STATE_HOME/<app>/updater.sentinel`
 *
 * `<app>` comes from `PERRY_APP_ID` env var, falling back to the basename
 * of the running exe. Apps SHOULD set `PERRY_APP_ID` so the path stays
 * stable across rename/relocation.
 */
export function getSentinelPath(): string;

/**
 * Atomically replace `targetPath` with `stagedPath`, keeping the displaced
 * version at `<target>.prev`. On Unix, ensures the new binary has 0o755
 * permissions. Returns 1 on success, 0 on any IO error (and attempts to
 * roll back step 1 on a failed step 2).
 */
export function installUpdate(stagedPath: string, targetPath: string): number;

/**
 * Restore `<target>.prev` over `target`, undoing a prior install.
 * Moves the current (likely-broken) target to `<target>.broken` first as
 * a safety net. Returns 1 on success, 0 if no backup exists.
 */
export function performRollback(targetPath: string): number;

/**
 * Spawn `exePath` as a fully detached child process and return the child's
 * PID, or -1 on failure. The caller is expected to call `process.exit(0)`
 * shortly after — that's how the running process hands off to the new one.
 */
export function relaunch(exePath: string): number;
