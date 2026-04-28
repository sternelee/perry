// @perry/updater — high-level auto-updater for Perry desktop apps.
//
// Orchestrates: manifest fetch → semver compare → binary download →
// SHA-256 verify → Ed25519 verify → atomic install → detached relaunch,
// plus boot-time rollback on crash-loop detection.
//
// Built on the `perry/updater` ambient primitives (see types/perry/updater)
// and existing `fetch()` + `fs` for the network and disk pieces.

import {
  compareVersions,
  verifyHash,
  verifySignature,
  computeFileSha256,
  writeSentinel,
  readSentinel,
  clearSentinel,
  getExePath,
  getSentinelPath,
  installUpdate as nativeInstallUpdate,
  performRollback as nativePerformRollback,
  relaunch as nativeRelaunch,
} from "perry/updater";

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

export interface PlatformAsset {
  url: string;
  sha256: string;
  signature: string;
  size: number;
}

export interface UpdateManifest {
  schemaVersion: number;
  version: string;
  pubDate: string;
  notes: string;
  platforms: { [target: string]: PlatformAsset };
}

export interface Update {
  /** Target version offered by the manifest. */
  version: string;
  /** Release notes (Markdown). */
  notes: string;
  /** Asset metadata for the current platform. */
  asset: PlatformAsset;
  /** Where the staged binary will be written before install. */
  stagedPath: string;
  /** Final target where the running exe lives. */
  targetPath: string;
  /** Download the binary, verifying hash + signature. */
  download(onProgress?: (downloaded: number, total: number) => void): Promise<void>;
  /** Atomically replace the running exe and relaunch detached. Calls process.exit. */
  installAndRelaunch(): Promise<never>;
}

export interface UpdaterOptions {
  /** Manifest URL (HTTPS). */
  manifestUrl: string;
  /** Base64-encoded Ed25519 public key (32 bytes raw). */
  publicKey: string;
  /** Currently-installed version (semver). */
  currentVersion: string;
}

export interface InitOptions {
  /** Auto-rollback after a crash-loop. Default: true. */
  autoRollback?: boolean;
  /** Time after which a fresh install is considered "healthy" and the sentinel is cleared. Default: 60_000 ms. */
  healthCheckMs?: number;
  /** Restart count threshold past which we treat the new version as broken. Default: 2. */
  crashLoopThreshold?: number;
}

// ---------------------------------------------------------------------------
// Platform key
// ---------------------------------------------------------------------------

function platformKey(): string {
  // os.platform() returns "darwin" / "linux" / "win32"; os.arch() returns
  // "x64" / "arm64" / "ia32". Manifest uses canonical Rust-style triples.
  const platform = (globalThis as any).process?.platform ?? "";
  const arch = (globalThis as any).process?.arch ?? "";
  const os =
    platform === "darwin" ? "darwin" :
    platform === "win32" ? "windows" :
    "linux";
  const a =
    arch === "arm64" ? "aarch64" :
    arch === "x64" ? "x86_64" :
    arch === "ia32" ? "i686" :
    arch;
  return `${os}-${a}`;
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/**
 * Fetch the manifest, compare against `currentVersion`, and return an
 * `Update` handle if a newer version is available for this platform.
 * Returns null when up to date or no asset is published for this platform.
 */
export async function checkForUpdate(opts: UpdaterOptions): Promise<Update | null> {
  const res = await fetch(opts.manifestUrl);
  if (!res.ok) {
    throw new Error(`updater: manifest fetch failed: ${res.status}`);
  }
  const manifest = (await res.json()) as UpdateManifest;

  if (manifest.schemaVersion !== 1) {
    throw new Error(`updater: unsupported manifest schemaVersion ${manifest.schemaVersion}`);
  }

  const cmp = compareVersions(opts.currentVersion, manifest.version);
  if (cmp === -2) throw new Error(`updater: invalid version string`);
  if (cmp >= 0) return null; // up to date or downgrade — never offered

  const key = platformKey();
  const asset = manifest.platforms[key];
  if (!asset) return null;

  const targetPath = getExePath();
  const stagedPath = `${targetPath}.staged`;

  return {
    version: manifest.version,
    notes: manifest.notes,
    asset,
    stagedPath,
    targetPath,
    async download(onProgress) {
      await downloadAndVerify(asset, stagedPath, opts.publicKey, onProgress);
    },
    async installAndRelaunch() {
      await applyAndRelaunch(stagedPath, targetPath, opts.currentVersion, manifest.version);
      // applyAndRelaunch never returns — process.exit() inside.
      throw new Error("unreachable");
    },
  };
}

/**
 * Boot-time hook: detect failed prior installs and roll back if the new
 * version appears to be crash-looping. Call this near the top of `main()`,
 * right after process initialization.
 *
 * Lifecycle:
 *  - No sentinel:       first boot or clean state → no-op.
 *  - Sentinel "armed":  we are the new version; increment restart count,
 *                       arm a `healthCheckMs` timer to clear the sentinel
 *                       once we look healthy, and register a graceful-exit
 *                       hook so a quick close-and-reopen pattern doesn't
 *                       look like a crash loop.
 *  - Sentinel armed and `restartCount >= crashLoopThreshold`:
 *                       crash loop detected → `performRollback()` and
 *                       `process.exit(0)` so the launcher restarts us at
 *                       the rolled-back binary.
 *
 * The graceful-exit hook is the difference between "user closed the app
 * within 60s" (legitimate, shouldn't bump the count) and "the new version
 * crashed during boot" (should). Without it, short-lived apps and CLIs
 * would false-positive their way into a rollback after two clean
 * close-and-reopen cycles.
 */
export async function initUpdater(options: InitOptions = {}): Promise<void> {
  const autoRollback = options.autoRollback ?? true;
  const healthCheckMs = options.healthCheckMs ?? 60_000;
  const threshold = options.crashLoopThreshold ?? 2;

  if (!autoRollback) return;

  const sentinelPath = getSentinelPath();
  const raw = readSentinel(sentinelPath);
  if (!raw) return;

  let state: SentinelPayload;
  try {
    state = JSON.parse(raw) as SentinelPayload;
  } catch {
    // Malformed sentinel — clear it so we don't retry forever.
    clearSentinel(sentinelPath);
    return;
  }

  if (state.state !== "armed") return;

  state.restartCount = (state.restartCount ?? 0) + 1;

  if (state.restartCount >= threshold) {
    // Crash loop — roll back and bail.
    nativePerformRollback(getExePath());
    clearSentinel(sentinelPath);
    (globalThis as any).process?.exit?.(0);
    return;
  }

  // Persist the bumped count, then arm the two paths that can clear the
  // sentinel without triggering a rollback on the next boot:
  //  1. Health-check timer fires after a quiet window — the new version
  //     stayed alive long enough that we trust it.
  //  2. The user gracefully exits before the timer fires — close-and-reopen
  //     is a normal pattern for CLIs and quick-launch GUIs and shouldn't
  //     count toward crash-loop detection.
  writeSentinel(sentinelPath, JSON.stringify(state));
  setTimeout(() => clearSentinel(sentinelPath), healthCheckMs);

  // Register a graceful-exit hook if `process.on` is wired in this build.
  // The check keeps initUpdater portable across UI / CLI / minimal targets
  // — if the runtime doesn't expose process events, the timer is the only
  // path and the user can call `markHealthy()` explicitly instead.
  const proc = (globalThis as any).process;
  if (proc && typeof proc.on === "function") {
    proc.on("exit", () => clearSentinel(sentinelPath));
  }
}

/**
 * Explicitly mark the running version as healthy and clear the sentinel.
 *
 * `initUpdater` already arms a timer + a graceful-exit hook, so most apps
 * never need to call this. Reach for it when:
 *
 *  - Your app passes its own integrity check earlier than the 60s default
 *    timer (e.g. a successful login, a database migration completed).
 *  - You're on a runtime where `process.on('exit', ...)` isn't wired,
 *    and you want to clear the sentinel on your own shutdown path.
 *  - You're writing a UI app and prefer to call this from `onTerminate`
 *    rather than relying on the generic exit hook.
 */
export function markHealthy(): void {
  clearSentinel(getSentinelPath());
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

interface SentinelPayload {
  prevExePath: string;
  stagedAt: string;
  currentVersion: string;
  targetVersion: string;
  restartCount: number;
  state: "installing" | "armed";
}

async function downloadAndVerify(
  asset: PlatformAsset,
  stagedPath: string,
  publicKey: string,
  onProgress?: (downloaded: number, total: number) => void,
): Promise<void> {
  const res = await fetch(asset.url);
  if (!res.ok) {
    throw new Error(`updater: download failed: ${res.status}`);
  }
  const total = asset.size;
  // For a streaming-friendly variant use res.body when it's wired; for v1
  // we accept the simpler buffer-the-whole-payload shape since Perry
  // binaries are tens of MB at most.
  const buf = await res.arrayBuffer();
  if (onProgress) onProgress(buf.byteLength, total);

  // Write to staged path atomically: tmp file + rename. Bare fs.writeFileSync
  // is not atomic on its own, so we tmp + rename to make the staged binary
  // appear in one filesystem step.
  //
  // Note: `Buffer.from(arrayBuffer)` is required here. Passing a `Uint8Array`
  // built from the same buffer ends up taking Perry's "string write" path
  // and only the first byte hits disk — separate runtime issue surfaced by
  // the smoke test, easy to step on without realising.
  const fs = await import("fs");
  const tmp = `${stagedPath}.tmp`;
  fs.writeFileSync(tmp, Buffer.from(buf) as any);
  fs.renameSync(tmp, stagedPath);

  if (verifyHash(stagedPath, asset.sha256) !== 1) {
    const actual = computeFileSha256(stagedPath);
    throw new Error(
      `updater: SHA-256 mismatch — expected ${asset.sha256}, got ${actual}`,
    );
  }
  if (verifySignature(stagedPath, asset.signature, publicKey) !== 1) {
    throw new Error(`updater: Ed25519 signature verification failed`);
  }
}

async function applyAndRelaunch(
  stagedPath: string,
  targetPath: string,
  currentVersion: string,
  targetVersion: string,
): Promise<void> {
  const sentinelPath = getSentinelPath();
  const prevPath = `${targetPath}.prev`;

  // Arm the sentinel BEFORE we touch the binary. If the install crashes
  // partway, the next boot will see the sentinel and either retry health
  // check or roll back.
  const sentinel: SentinelPayload = {
    prevExePath: prevPath,
    stagedAt: new Date().toISOString(),
    currentVersion,
    targetVersion,
    restartCount: 0,
    state: "armed",
  };
  if (writeSentinel(sentinelPath, JSON.stringify(sentinel)) !== 1) {
    throw new Error("updater: failed to write sentinel");
  }

  if (nativeInstallUpdate(stagedPath, targetPath) !== 1) {
    clearSentinel(sentinelPath);
    throw new Error("updater: install failed");
  }

  if (nativeRelaunch(targetPath) < 0) {
    // Relaunch failed — the install already happened, so we're stuck on
    // the new version. Don't roll back here; let the user retry manually.
    throw new Error("updater: relaunch failed (install committed; restart manually)");
  }

  (globalThis as any).process?.exit?.(0);
}
