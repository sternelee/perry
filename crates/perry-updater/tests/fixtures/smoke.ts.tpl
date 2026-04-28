// Smoke-test fixture for the perry/updater happy path.
//
// Compiled twice by scripts/smoke_updater.sh — once with __VERSION__
// substituted as "1.0.0" and once as "1.0.1", same Ed25519 pubkey baked
// into both. The 1.0.0 build drives the update; the 1.0.1 build only
// proves it ran by appending its version to a marker file.
//
// What this smoke covers: verify (SHA-256 + Ed25519) → installUpdate
// (atomic rename + .prev backup) → relaunch (detached spawn).
// What it does NOT cover: the network download of the new binary into
// the staged path. Perry's `await response.arrayBuffer()` currently
// returns a metadata-only object (just `{ byteLength }`) — the actual
// body bytes never reach TS, so `fs.writeFileSync(path, Buffer.from(buf))`
// drops every byte but the first. The smoke shell pre-stages v1.0.1 via
// `cp` so we can exercise the rest of the flow end-to-end while that
// arrayBuffer-to-disk gap is tracked separately.

import {
  compareVersions,
  verifyHash,
  verifySignature,
  installUpdate,
  relaunch,
  getExePath,
} from "perry/updater";

import * as fs from "fs";

const VERSION = "__VERSION__";
const PUBKEY = "__PUBKEY__";

// First thing we always do: append our version to the marker file so the
// outer shell script can confirm both 1.0.0 and 1.0.1 ran in order.
// (Perry's `fs.appendFileSync` currently writes 0 bytes, so we manually
// read+concat+write instead — separate runtime issue.)
const marker = process.env.SMOKE_MARKER ?? "";
if (marker) {
  const existing = fs.existsSync(marker) ? fs.readFileSync(marker, "utf8") : "";
  fs.writeFileSync(marker, existing + VERSION + "\n");
}

// 1.0.1 has nothing else to do. It exists only to be installed and
// relaunched and to leave its mark on the file.
if (VERSION !== "1.0.0") {
  process.exit(0);
}

// 1.0.0: drive the actual update flow. Manifest URL comes in via env so the
// shell can choose a port without rebuilding.
const manifestUrl = process.env.SMOKE_MANIFEST_URL ?? "";
if (!manifestUrl) {
  console.error("SMOKE_MANIFEST_URL not set");
  process.exit(2);
}

async function main(): Promise<void> {
  // While iterating on this fixture I hit intermittent hangs at the first
  // `await fetch(...)` that disappeared once the surrounding shape settled
  // into what's here now. I have multiple minimal repros (top-level
  // env read + simpler-than-this main → fetch never sends the request)
  // but couldn't isolate the exact trigger — it's not the env-at-top-level
  // alone, since this fixture does that and works. Worth filing as a
  // separate Perry runtime issue once someone with more context bisects
  // the async state-machine codegen properly.
  console.log("[smoke v1.0.0] fetching manifest from", manifestUrl);
  const res = await fetch(manifestUrl);
  if (!res.ok) {
    console.error("manifest fetch failed:", res.status);
    process.exit(3);
  }
  console.log("[smoke v1.0.0] manifest status =", res.status);
  const manifest = (await res.json()) as any;
  console.log("[smoke v1.0.0] manifest version =", manifest.version);

  // The smoke server only publishes one platform/arch pair, matching the
  // host that's running this script — pick whichever entry exists.
  const keys = Object.keys(manifest.platforms);
  if (keys.length === 0) {
    console.error("manifest has no platforms");
    process.exit(4);
  }
  const asset = manifest.platforms[keys[0]];

  const cmp = compareVersions(VERSION, manifest.version);
  if (cmp !== -1) {
    console.error("expected an update to be available, got cmp =", cmp);
    process.exit(5);
  }

  // The shell pre-staged v1.0.1 at <exe>.staged for us — see file header.
  // Verify hash + signature against the pre-staged file as if we'd just
  // downloaded it.
  const exePath = getExePath();
  const stagedPath = exePath + ".staged";
  if (!fs.existsSync(stagedPath)) {
    console.error("pre-staged file missing at", stagedPath);
    process.exit(6);
  }
  const stagedSize = fs.statSync(stagedPath).size;
  console.log("[smoke v1.0.0] pre-staged bytes =", stagedSize);

  const hashOk = verifyHash(stagedPath, asset.sha256);
  console.log("[smoke v1.0.0] hash verify =", hashOk);
  if (hashOk !== 1) {
    console.error("hash verify failed");
    process.exit(7);
  }
  const sigOk = verifySignature(stagedPath, asset.signature, PUBKEY);
  console.log("[smoke v1.0.0] sig verify =", sigOk);
  if (sigOk !== 1) {
    console.error("signature verify failed");
    process.exit(8);
  }

  const installOk = installUpdate(stagedPath, exePath);
  console.log("[smoke v1.0.0] install =", installOk);
  if (installOk !== 1) {
    console.error("install failed");
    process.exit(9);
  }

  const pid = relaunch(exePath);
  console.log("[smoke v1.0.0] relaunched pid =", pid);
  if (pid < 0) {
    console.error("relaunch failed");
    process.exit(10);
  }

  // Hand off to the new process. The detached child is now writing to the
  // marker file; this process just needs to get out of the way.
  process.exit(0);
}

main().catch((err) => {
  console.error("smoke main threw:", err);
  process.exit(99);
});
