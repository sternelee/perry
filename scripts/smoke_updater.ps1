# End-to-end smoke test for perry/updater on Windows.
#
# Mirror of scripts/smoke_updater.sh. Drives the same flow: build two
# versions of a tiny test program, sign v1.0.1 with a fresh Ed25519
# keypair (via openssl), serve a manifest over HTTP from python, run
# v1.0.0, assert v1.0.1 boots after the install.
#
# Out of scope (each is a separate follow-up):
#   - AppImage (Linux-only — no Windows equivalent)
#   - Crash-loop rollback (separate test, independent code path)
#   - CI integration (updater smokes are timing-sensitive enough that
#     manual pre-release runs catch more than green CI does)
#
# Usage:
#   scripts/smoke_updater.ps1
#   $env:PERRY_BIN = 'C:\path\to\perry.exe'
#   $env:SMOKE_PORT = '18765'
#   scripts/smoke_updater.ps1
#
# Exit codes: 0 on success, non-zero with a diagnostic on any failure step.

$ErrorActionPreference = 'Stop'

# ---------------------------------------------------------------------------
# Locate perry + sanity-check tools.
# ---------------------------------------------------------------------------
$RepoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
$PerryBin = if ($env:PERRY_BIN) { $env:PERRY_BIN } else { Join-Path $RepoRoot 'target\release\perry.exe' }
$Port     = if ($env:SMOKE_PORT) { [int]$env:SMOKE_PORT } else { 18765 }

if (-not (Test-Path $PerryBin)) {
    Write-Error "perry binary not found at $PerryBin. Set `$env:PERRY_BIN or run 'cargo build --release -p perry'."
    exit 2
}
foreach ($tool in @('openssl', 'python')) {
    if (-not (Get-Command $tool -ErrorAction SilentlyContinue)) {
        Write-Error "required tool not found on PATH: $tool"
        exit 2
    }
}

# Detect arch — Windows arm64 reports as ARM64, x86_64 as AMD64.
$Arch = switch -Wildcard ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()) {
    'X64'   { 'x86_64' }
    'Arm64' { 'aarch64' }
    default { Write-Error "unsupported arch: $_"; exit 2 }
}
$PlatformKey = "windows-$Arch"

$Fixture = Join-Path $RepoRoot 'crates\perry-updater\tests\fixtures\smoke.ts.tpl'
if (-not (Test-Path $Fixture)) {
    Write-Error "fixture not found at $Fixture"
    exit 2
}

$Tmp = Join-Path $env:TEMP "perry-updater-smoke-$([guid]::NewGuid().ToString('N').Substring(0,8))"
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null

$ServerProc = $null
$Cleanup = {
    if ($ServerProc -and -not $ServerProc.HasExited) {
        try { Stop-Process -Id $ServerProc.Id -Force -ErrorAction SilentlyContinue } catch {}
    }
    if (-not $env:SMOKE_KEEP) {
        Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
    } else {
        Write-Host "(SMOKE_KEEP set — leaving $Tmp for inspection)"
    }
}
trap { & $Cleanup; break }

Write-Host "==> tmp dir: $Tmp"
Write-Host "==> platform: $PlatformKey"

# ---------------------------------------------------------------------------
# 1. Throwaway Ed25519 keypair.
# ---------------------------------------------------------------------------
Write-Host '==> generating Ed25519 keypair'
$PrivDer = Join-Path $Tmp 'priv.der'
$PubDer  = Join-Path $Tmp 'pub.der'
& openssl genpkey -algorithm ED25519 -outform DER -out $PrivDer 2>$null
if ($LASTEXITCODE -ne 0) { Write-Error 'openssl genpkey failed'; & $Cleanup; exit 1 }
& openssl pkey -in $PrivDer -inform DER -pubout -outform DER -out $PubDer 2>$null
$PubBytes = [System.IO.File]::ReadAllBytes($PubDer)
$PubKey32 = $PubBytes[($PubBytes.Length - 32)..($PubBytes.Length - 1)]
$PubKeyB64 = [Convert]::ToBase64String($PubKey32)

# ---------------------------------------------------------------------------
# 2. Render the fixture as v1.0.0 and v1.0.1, then compile both.
# ---------------------------------------------------------------------------
Write-Host '==> compiling test binaries'
$BuildDir = Join-Path $Tmp 'build'
New-Item -ItemType Directory -Force -Path $BuildDir | Out-Null

$Template = Get-Content $Fixture -Raw
foreach ($v in @('1.0.0', '1.0.1')) {
    $Rendered = $Template.Replace('__VERSION__', $v).Replace('__PUBKEY__', $PubKeyB64)
    $TsPath  = Join-Path $BuildDir "smoke_$v.ts"
    $ExePath = Join-Path $BuildDir "v$v.exe"
    Set-Content -Path $TsPath -Value $Rendered -Encoding UTF8 -NoNewline
    & $PerryBin compile $TsPath -o $ExePath 2> (Join-Path $BuildDir "v$v.log")
    if ($LASTEXITCODE -ne 0) {
        Write-Error "compile of v$v failed"
        Get-Content (Join-Path $BuildDir "v$v.log") | Write-Host
        & $Cleanup; exit 1
    }
}

# ---------------------------------------------------------------------------
# 3. Sign v1.0.1.
# ---------------------------------------------------------------------------
# Pure Ed25519 over the raw 32-byte SHA-256 digest of the binary — same sig
# domain as the bash sibling. -rawin tells openssl pkeyutl not to apply its
# own hashing; the bytes we hand it are exactly what gets signed.
Write-Host '==> signing v1.0.1'
$BinV1 = Join-Path $BuildDir 'v1.0.1.exe'
$Digest = Join-Path $Tmp 'digest.bin'
$SigBin = Join-Path $Tmp 'sig.bin'
& openssl dgst -sha256 -binary -out $Digest $BinV1 2>$null
& openssl pkeyutl -sign -inkey $PrivDer -keyform DER -rawin -in $Digest -out $SigBin 2>$null
$SigBytes = [System.IO.File]::ReadAllBytes($SigBin)
$SigB64 = [Convert]::ToBase64String($SigBytes)
$Sha256Hex = (Get-FileHash $BinV1 -Algorithm SHA256).Hash.ToLower()
$Size = (Get-Item $BinV1).Length

# ---------------------------------------------------------------------------
# 4. Manifest + serve.
# ---------------------------------------------------------------------------
$ServerDir = Join-Path $Tmp 'server'
New-Item -ItemType Directory -Force -Path $ServerDir | Out-Null
Copy-Item $BinV1 (Join-Path $ServerDir 'v1.0.1.exe')
$Manifest = @"
{
  "schemaVersion": 1,
  "version": "1.0.1",
  "pubDate": "2026-04-27T00:00:00Z",
  "notes": "smoke test fixture",
  "platforms": {
    "$PlatformKey": {
      "url": "http://127.0.0.1:$Port/v1.0.1.exe",
      "sha256": "$Sha256Hex",
      "signature": "$SigB64",
      "size": $Size
    }
  }
}
"@
Set-Content -Path (Join-Path $ServerDir 'manifest.json') -Value $Manifest -Encoding UTF8

Write-Host "==> starting http server on :$Port"
$ServerProc = Start-Process -FilePath python -ArgumentList @('-m', 'http.server', $Port, '--bind', '127.0.0.1') `
    -WorkingDirectory $ServerDir -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $ServerDir 'access.log')

# Wait for the server to start accepting connections.
$ready = $false
for ($i = 0; $i -lt 30; $i++) {
    try {
        Invoke-WebRequest "http://127.0.0.1:$Port/manifest.json" -UseBasicParsing -TimeoutSec 1 | Out-Null
        $ready = $true; break
    } catch { Start-Sleep -Milliseconds 200 }
}
if (-not $ready) {
    Write-Error "http server didn't come up on port $Port"
    & $Cleanup; exit 1
}

# ---------------------------------------------------------------------------
# 5. Stage v1.0.0 + pre-stage v1.0.1 (see fixture file header for why we
#    skip the runtime-network-download path in this smoke).
# ---------------------------------------------------------------------------
$InstallDir = Join-Path $Tmp 'installed'
New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
$InstalledExe = Join-Path $InstallDir 'perry-smoke-app.exe'
Copy-Item (Join-Path $BuildDir 'v1.0.0.exe') $InstalledExe
Copy-Item $BinV1 "$InstalledExe.staged"

$Marker = Join-Path $Tmp 'marker.txt'
Set-Content -Path $Marker -Value '' -NoNewline

$env:SMOKE_MARKER       = $Marker
$env:SMOKE_MANIFEST_URL = "http://127.0.0.1:$Port/manifest.json"

Write-Host '==> running v1.0.0 (will install + relaunch v1.0.1)'
$StdoutFile = Join-Path $Tmp 'v1.0.0.stdout'
$StderrFile = Join-Path $Tmp 'v1.0.0.stderr'
$Run = Start-Process -FilePath $InstalledExe -PassThru -Wait `
    -RedirectStandardOutput $StdoutFile -RedirectStandardError $StderrFile -NoNewWindow
$Rc = $Run.ExitCode
if ($Rc -ne 0) {
    Write-Error "v1.0.0 exited with $Rc"
    Write-Host '--- stdout ---'; Get-Content $StdoutFile | Write-Host
    Write-Host '--- stderr ---'; Get-Content $StderrFile | Write-Host
    & $Cleanup; exit 1
}

# ---------------------------------------------------------------------------
# 6. Wait for v1.0.1 to write its marker line.
# ---------------------------------------------------------------------------
for ($i = 0; $i -lt 50; $i++) {
    if (Select-String -Path $Marker -Pattern '^1\.0\.1$' -Quiet) { break }
    Start-Sleep -Milliseconds 200
}

# ---------------------------------------------------------------------------
# 7. Assertions.
# ---------------------------------------------------------------------------
$Ok = $true
if (-not (Select-String -Path $Marker -Pattern '^1\.0\.0$' -Quiet)) {
    Write-Host 'FAIL: v1.0.0 did not record its run in the marker'
    $Ok = $false
}
if (-not (Select-String -Path $Marker -Pattern '^1\.0\.1$' -Quiet)) {
    Write-Host 'FAIL: v1.0.1 never appeared in the marker (relaunch did not take)'
    $Ok = $false
}

# Sanity: the binary at $InstalledExe should now be v1.0.1's bytes.
$InstalledHash = (Get-FileHash $InstalledExe -Algorithm SHA256).Hash.ToLower()
if ($InstalledHash -ne $Sha256Hex) {
    Write-Host "FAIL: installed binary hash $InstalledHash != expected v1.0.1 $Sha256Hex"
    $Ok = $false
}
# Sanity: the .prev backup should be present.
if (-not (Test-Path "$InstalledExe.prev")) {
    Write-Host "FAIL: $InstalledExe.prev backup missing"
    $Ok = $false
}

if (-not $Ok) {
    Write-Host '--- marker ---'; Get-Content $Marker | Write-Host
    Write-Host '--- v1.0.0 stdout ---'; Get-Content $StdoutFile | Write-Host
    Write-Host '--- v1.0.0 stderr ---'; Get-Content $StderrFile | Write-Host
    & $Cleanup; exit 1
}

Write-Host ''
Write-Host 'smoke test PASSED'
Write-Host '  marker:'
Get-Content $Marker | ForEach-Object { Write-Host "    $_" }
& $Cleanup
exit 0
