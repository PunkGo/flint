# flint suite bootstrap — thin glue, no logic (suite plan v4.3 E2). Windows entrypoint.
# Prereq (README): git clone <repo>; cd flint
# Usage: scripts/bootstrap.ps1 [-Stage skills|full] [-Instance <git-url>] [-Root <dir>]
#
# Mirror of bootstrap.sh: build, provision the memory-instance pointer (opt-in
# via -Instance: clone if absent + write ~/.flint/instance.toml), preflight
# (stage full only), hand off to `flint install`. Keep it dumb — semantics live
# in the binary. The instance URL is YOUR private repo — passed in, never baked
# into this open repo. Without -Instance the suite still installs; $INSTANCE
# entries skip with a warning.
param(
  [ValidateSet("skills", "full")]
  [string]$Stage = "skills",
  [string]$Instance = "",
  [string]$Root = ""
)
$ErrorActionPreference = "Stop"

$RepoDir = Split-Path -Parent $PSScriptRoot

cargo build --release --manifest-path (Join-Path $RepoDir "Cargo.toml")
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
$Bin = Join-Path $RepoDir "target\release\flint.exe"

# ---- memory-instance provisioning (opt-in, one-time, idempotent) ----
$InstanceToml = Join-Path $env:USERPROFILE ".flint\instance.toml"
if ($Instance -ne "") {
  if (Test-Path $InstanceToml) {
    Write-Host "bootstrap: $InstanceToml already exists — leaving your pointer untouched"
  } else {
    if ($Root -eq "") { $Root = Join-Path $env:USERPROFILE "memory" }
    if (-not (Test-Path (Join-Path $Root ".git"))) {
      git clone $Instance $Root
      if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } else {
      Write-Host "bootstrap: $Root already a git repo — skipping clone, writing pointer only"
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $env:USERPROFILE ".flint") | Out-Null
    $Machine = $env:COMPUTERNAME.ToLower()
    @(
      "# flint memory-instance pointer (written by bootstrap; judge/hook never read this)."
      "root    = `"$($Root -replace '\\','/')`""
      "machine = `"$Machine`""
      "repo    = `"$($RepoDir -replace '\\','/')`""
    ) | Set-Content -Path $InstanceToml -Encoding utf8
    Write-Host "bootstrap: wrote $InstanceToml (root=$Root, machine=$Machine)"
  }
}

# The runtime config the suite renders from — single source for BOTH the
# full-stage preflight AND the install hand-off. WP-G P0: the hand-off used to
# omit --config, so a full-stage bootstrap always aborted with "generator
# entries in scope — pass --config". Skills stage never reads it, so passing it
# unconditionally is safe and keeps one code path.
$FlintConfig = Join-Path $env:USERPROFILE ".flint\flint-global.toml"

# Stage full renders harness bindings from the signed Canon: refuse on an
# unsigned/invalid canon (Eng-2). Skills stage skips the check.
if ($Stage -eq "full") {
  & $Bin canon list --config $FlintConfig | Out-Null
  if ($LASTEXITCODE -ne 0) {
    Write-Error "bootstrap: canon not signed/valid — run 'flint canon pick' first (stage full renders from the signed canon)"
    exit 1
  }
}

& $Bin install --manifest (Join-Path $RepoDir "scripts\manifest.toml") --stage $Stage --config $FlintConfig
exit $LASTEXITCODE
