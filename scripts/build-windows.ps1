# Build Windows distributables for Tasty.
#
# Usage:
#   .\scripts\build-windows.ps1           # dist build (full LTO, 배포용)
#   .\scripts\build-windows.ps1 -Release  # release build (thin LTO, 빠른 빌드)
#   .\scripts\build-windows.ps1 -Debug    # debug build
#   .\scripts\build-windows.ps1 -SkipMsi  # ZIP만 만들고 MSI는 건너뜀
#
# Output:
#   dist\tasty-{version}-windows-x64.zip   (portable)
#   dist\tasty-{version}-windows-x64.msi   (installer; cargo-wix + WiX 3.x auto-installed if missing)

param(
    [switch]$Release,
    [switch]$Debug,
    [switch]$SkipMsi
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if ($env:OS -ne "Windows_NT") {
    Write-Error "This script must be run on Windows."
    exit 1
}

Push-Location (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path))
try {

# Parse profile
# NOTE: $Profile is a PowerShell automatic variable (user profile script path).
# Use $BuildProfile to avoid the name collision.
$BuildProfile = "dist"
$CargoFlags = @("--profile", "dist")
if ($Debug) {
    $BuildProfile = "debug"
    $CargoFlags = @()
} elseif ($Release) {
    $BuildProfile = "release"
    $CargoFlags = @("--release")
}

# Extract version from Cargo.toml
$CargoContent = Get-Content "Cargo.toml" -Raw
if ($CargoContent -match '(?m)^version\s*=\s*"([^"]+)"') {
    $Version = $Matches[1]
} else {
    Write-Error "Failed to extract version from Cargo.toml"
    exit 1
}

$DistDir = "dist"
$ArchiveName = "tasty-${Version}-windows-x64.zip"
$StageDir = Join-Path $DistDir "tasty-windows"

# Resolve a usable bash for the POSIX helper scripts. Prefer **Git Bash** — on
# Windows a bare `bash` frequently resolves to WSL's bash, which fails for these
# scripts (no distro installed / relay error: "execvpe(/bin/bash) failed").
function Resolve-Bash {
    $candidates = @()
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd) {
        # .../Git/cmd/git.exe -> .../Git
        $gitRoot = Split-Path (Split-Path $gitCmd.Source -Parent) -Parent
        $candidates += (Join-Path $gitRoot 'bin\bash.exe')
        $candidates += (Join-Path $gitRoot 'usr\bin\bash.exe')
    }
    $candidates += 'C:\Program Files\Git\bin\bash.exe'
    $candidates += 'C:\Program Files\Git\usr\bin\bash.exe'
    if (${env:ProgramFiles(x86)}) {
        $candidates += (Join-Path ${env:ProgramFiles(x86)} 'Git\bin\bash.exe')
    }
    foreach ($c in $candidates) {
        if ($c -and (Test-Path $c)) { return $c }
    }
    # Last resort: bare bash from PATH (may be WSL, but better than nothing).
    $b = Get-Command bash -ErrorAction SilentlyContinue
    if ($b) { return $b.Source }
    return $null
}

# Discover signing key BEFORE cargo build — so that the matching dev-pubkey.bin
# is (re)derived and embedded into the host-plugin binary at compile time.
if ($BuildProfile -ne "debug") {
    $SignKeyPath = $env:SIGN_KEY_PATH
    if (-not $SignKeyPath) {
        $ReleaseKey = Join-Path $env:USERPROFILE ".tasty-keys\release.pem"
        $DevKey = Join-Path $env:USERPROFILE ".tasty-keys\dev.pem"
        if (Test-Path $ReleaseKey) {
            $SignKeyPath = $ReleaseKey
        } else {
            # dev 키 경로: 없으면 생성, 있으면 추적되지 않는 dev-pubkey.bin 을
            # dev.pem 에서 재도출한다. gen-dev-key.sh 가 두 경우 모두 처리
            # (idempotent) → 빌드가 all-zero placeholder 대신 서명 키와 일치하는
            # trust 키를 임베드한다.
            Write-Host "==> Ensuring dev signing key + embedded pubkey..."
            $Bash = Resolve-Bash
            if (-not $Bash) {
                Write-Error "Git Bash not found. Install Git for Windows to run scripts/gen-dev-key.sh."
                exit 1
            }
            & $Bash ./scripts/gen-dev-key.sh
            if ($LASTEXITCODE -ne 0) {
                Write-Error "gen-dev-key.sh failed with exit code $LASTEXITCODE"
                exit 1
            }
            $SignKeyPath = $DevKey
        }
    }
    $env:SIGN_KEY_PATH = $SignKeyPath
}

Write-Host "==> Building tasty ($BuildProfile)..."
cargo build @CargoFlags
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed with exit code $LASTEXITCODE"
    exit 1
}

# Discover bundled plugin crates (any `crates\tasty-plugin-*` with a manifest).
# Matches build-linux.sh / build-macos-dmg.sh — keep in sync. A manifest with
# `bundle = false` (demo/PoC plugins) is skipped from distribution; dev staging
# (`just build-plugins`/`link-plugins`) still includes it.
$PluginCrates = @()
foreach ($d in (Get-ChildItem -Path "crates" -Filter "tasty-plugin-*" -Directory)) {
    $manifest = Join-Path $d.FullName "tasty-plugin.toml"
    if (Test-Path $manifest) {
        if (Select-String -Path $manifest -Pattern '^\s*bundle\s*=\s*false' -Quiet) {
            Write-Host "==> Skipping $($d.Name) (bundle = false)"
            continue
        }
        $PluginCrates += $d.Name
    }
}

if ($PluginCrates.Count -eq 0) {
    Write-Error "No plugin crates with tasty-plugin.toml found under crates\"
    exit 1
}

Write-Host "==> Building $($PluginCrates.Count) plugins ($BuildProfile)..."
$PluginCargoArgs = @()
foreach ($c in $PluginCrates) {
    $PluginCargoArgs += @("-p", $c)
}
cargo build @CargoFlags @PluginCargoArgs
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build (plugins) failed with exit code $LASTEXITCODE"
    exit 1
}

# release/dist builds: sign all plugin manifests (Ed25519) with the key
# discovered (or auto-generated) before cargo build.
if ($BuildProfile -ne "debug") {
    $SignKeyPath = $env:SIGN_KEY_PATH
    $Bash = Resolve-Bash
    if (-not $Bash) {
        Write-Error "Git Bash not found. Install Git for Windows to run scripts/sign-bundle.sh."
        exit 1
    }
    Write-Host "==> Signing plugin manifests with $SignKeyPath..."
    & $Bash ./scripts/sign-bundle.sh --key $SignKeyPath --all-builtins
    if ($LASTEXITCODE -ne 0) {
        Write-Error "sign-bundle.sh failed with exit code $LASTEXITCODE"
        exit 1
    }
}

# Stage plugins under <dest>\<id>\. Mirrors macOS build-macos-dmg.sh staging.
# `bundle_root()` (crates\tasty-host-plugin\src\builtin.rs) discovers
# `<exe_dir>\plugins\` and syncs each `<plugin-id>\` into
# `%USERPROFILE%\.tasty\plugins\<id>\` on first launch.
function Stage-Plugins {
    param([string]$PluginsDir)
    New-Item -ItemType Directory -Force -Path $PluginsDir | Out-Null
    foreach ($c in $PluginCrates) {
        $manifest = Join-Path "crates" (Join-Path $c "tasty-plugin.toml")
        $idMatch = Select-String -Path $manifest -Pattern '^\s*id\s*=\s*"([^"]+)"' | Select-Object -First 1
        if (-not $idMatch) {
            Write-Error "Cannot parse id from $manifest"
            exit 1
        }
        $id = $idMatch.Matches[0].Groups[1].Value
        $srcBin = Join-Path (Join-Path "target" $BuildProfile) "$c.exe"
        if (-not (Test-Path $srcBin)) {
            Write-Error "Plugin binary missing: $srcBin"
            exit 1
        }
        $dest = Join-Path $PluginsDir $id
        New-Item -ItemType Directory -Force -Path $dest | Out-Null
        Copy-Item $srcBin -Destination (Join-Path $dest "$c.exe")
        Copy-Item $manifest -Destination (Join-Path $dest "tasty-plugin.toml")
        # .sig sidecar — produced by sign-bundle.sh above; required for non-debug
        # builds, optional otherwise (debug runtime warns instead of rejecting).
        $sigPath = Join-Path "crates" (Join-Path $c "tasty-plugin.toml.sig")
        if (Test-Path $sigPath) {
            Copy-Item $sigPath -Destination (Join-Path $dest "tasty-plugin.toml.sig")
        } elseif ($BuildProfile -ne "debug") {
            Write-Error "Missing $sigPath (signing failed?)"
            exit 1
        }
        $langDir = Join-Path "crates" (Join-Path $c "lang")
        if (Test-Path $langDir) {
            $destLang = Join-Path $dest "lang"
            if (Test-Path $destLang) { Remove-Item -Recurse -Force $destLang }
            Copy-Item -Recurse $langDir $destLang
        }
        Write-Host "  staged $id"
    }
}

# The notice set — LICENSE, THIRD_PARTY_LICENSES.md and every file directly
# under LICENSES\ — as repo-relative paths. THIRD_PARTY_LICENSES.md's "notice
# set" section is the definition; scripts/lib/notice-set.sh reads the directory
# the same way for the Linux and macOS artifacts. The set is read from the
# directory, not listed here, so a new licence text needs no edit to this file.
function Get-NoticeSetFiles {
    $files = @("LICENSE", "THIRD_PARTY_LICENSES.md")
    $texts = @(Get-ChildItem -Path "LICENSES" -File | Sort-Object Name)
    if ($texts.Count -eq 0) {
        Write-Error "LICENSES\ holds no licence text"
        exit 1
    }
    foreach ($t in $texts) { $files += (Join-Path "LICENSES" $t.Name) }
    return $files
}

# Copy the notice set into a distribution tree. LICENSES\ keeps its
# subdirectory so the relative links inside THIRD_PARTY_LICENSES.md resolve.
function Stage-Notice([string]$Dest) {
    New-Item -ItemType Directory -Force -Path (Join-Path $Dest "LICENSES") | Out-Null
    foreach ($f in Get-NoticeSetFiles) {
        Copy-Item $f -Destination (Join-Path $Dest $f)
    }
}

# Compare a staged or extracted tree with the repo copy, byte for byte.
function Test-NoticeTree([string]$Root, [string]$Label) {
    foreach ($f in Get-NoticeSetFiles) {
        $staged = Join-Path $Root $f
        if (-not (Test-Path $staged)) {
            Write-Error "$f missing from $Label"
            exit 1
        }
        $want = (Get-FileHash $f -Algorithm SHA256).Hash
        $got = (Get-FileHash $staged -Algorithm SHA256).Hash
        if ($want -ne $got) {
            Write-Error "$f in $Label differs from the repo copy"
            exit 1
        }
    }
}

Write-Host "==> Assembling archive..."
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

$ExePath = Join-Path (Join-Path "target" $BuildProfile) "tasty.exe"
if (-not (Test-Path $ExePath)) {
    Write-Error "Build output not found: $ExePath"
    exit 1
}
Copy-Item $ExePath -Destination $StageDir

# Collect any DLLs from the build output directory
$BuildDir = Join-Path "target" $BuildProfile
Get-ChildItem -Path $BuildDir -Filter "*.dll" | ForEach-Object {
    Copy-Item $_.FullName -Destination $StageDir
}

Stage-Plugins (Join-Path $StageDir "plugins")
# Notice set at the archive's top level, next to tasty.exe — the same place the
# Linux tar.gz puts it.
Stage-Notice $StageDir

Write-Host "==> Creating $ArchiveName..."
$ArchivePath = Join-Path $DistDir $ArchiveName
if (Test-Path $ArchivePath) { Remove-Item -Force $ArchivePath }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ArchivePath

Remove-Item -Recurse -Force $StageDir

Write-Host ""
Write-Host "Portable archive: $DistDir\$ArchiveName"

# === MSI installer (cargo-wix) ===
# The .msi ships plugins via explicit Component / File entries in
# wix\main.wxs (one per binary, manifest, and lang file). They install
# to `<APPLICATIONFOLDER>\bin\plugins\<id>\` next to tasty.exe so the
# runtime `bundle_root()` finds them via the exe-relative `plugins/`
# lookup. Keep the wxs plugin list in sync with BUILTINS in
# crates\tasty-host-plugin\src\builtin.rs.
if (-not $SkipMsi) {
    Write-Host ""
    Write-Host "==> Building MSI installer..."

    # cargo-wix는 release/dist 프로필만 지원 (debug는 의미 없음)
    if ($BuildProfile -eq "debug") {
        Write-Host "  (skipping MSI for debug build)"
    } else {
        # --- Ensure cargo-wix is installed (zero-touch, mirrors signing-key auto-gen) ---
        $cargoWix = Get-Command cargo-wix -ErrorAction SilentlyContinue
        if (-not $cargoWix) {
            Write-Host "==> cargo-wix not found — installing via 'cargo install cargo-wix'..."
            cargo install cargo-wix
            if ($LASTEXITCODE -ne 0) {
                Write-Error "cargo install cargo-wix failed with exit code $LASTEXITCODE"
                exit 1
            }
            $cargoWix = Get-Command cargo-wix -ErrorAction SilentlyContinue
            if (-not $cargoWix) {
                Write-Error "cargo-wix still not on PATH after install. Ensure ~\.cargo\bin is on PATH, then re-run."
                exit 1
            }
        }

        # --- Ensure WiX 3.x is installed and discoverable ---
        # WiX 3.x (winget package WiXToolset.WiXToolset) registers the WIX
        # environment variable but does NOT add WIX\bin to PATH, so cargo-wix
        # can't find candle.exe / light.exe. Prepend it for this process.
        if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
            $wixRoot = $env:WIX
            if (-not $wixRoot) {
                $wixRoot = [Environment]::GetEnvironmentVariable("WIX", "Machine")
            }
            $wixBin = if ($wixRoot) { Join-Path $wixRoot "bin" } else { $null }

            # Not installed yet — install via winget. WiX 3.14 REQUIRES admin:
            # it enables the NetFx3 Windows feature (DISM) and installs at machine
            # scope, neither of which a non-elevated shell can do. Self-elevate
            # just the install via UAC, then return here. A single UAC click is
            # the minimum Windows allows — there is no silent/no-prompt path.
            if (-not ($wixBin -and (Test-Path (Join-Path $wixBin "candle.exe")))) {
                Write-Host "==> WiX Toolset not found — installing via winget (a UAC admin prompt will appear)..."
                $winget = Get-Command winget -ErrorAction SilentlyContinue
                if (-not $winget) {
                    Write-Error "winget not found. Install WiX 3.x manually: winget install -e --id WiXToolset.WiXToolset"
                    exit 1
                }
                # Elevated child: enable NetFx3 (WiX 3.x dependency) then install WiX.
                $elevCmd = 'Enable-WindowsOptionalFeature -Online -FeatureName NetFx3 -All -NoRestart -ErrorAction SilentlyContinue | Out-Null; winget install -e --id WiXToolset.WiXToolset --accept-source-agreements --accept-package-agreements; exit $LASTEXITCODE'
                try {
                    $proc = Start-Process -FilePath "powershell.exe" -Verb RunAs -Wait -PassThru `
                        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $elevCmd)
                } catch {
                    Write-Error "Could not elevate to install WiX (UAC declined or unavailable): $($_.Exception.Message). Install manually: winget install -e --id WiXToolset.WiXToolset"
                    exit 1
                }
                # Don't hard-fail on the child's exit code (e.g. 'already installed'
                # returns non-zero); the candle.exe check below is the real gate.
                if ($proc.ExitCode -ne 0) {
                    Write-Warning "Elevated WiX install returned exit code $($proc.ExitCode) — verifying candle.exe anyway..."
                }
                # winget registers WIX at Machine scope but not in this process —
                # re-read it live from the registry.
                $wixRoot = [Environment]::GetEnvironmentVariable("WIX", "Machine")
                if (-not $wixRoot) {
                    $wixRoot = [Environment]::GetEnvironmentVariable("WIX", "User")
                }
                $wixBin = if ($wixRoot) { Join-Path $wixRoot "bin" } else { $null }
            }

            if ($wixBin -and (Test-Path (Join-Path $wixBin "candle.exe"))) {
                $env:Path = "$wixBin;$env:Path"
                Write-Host "  (added $wixBin to PATH for cargo-wix)"
            } else {
                Write-Error "WiX candle.exe not found after install. The WIX env var is set at machine scope on install — open a new shell and re-run so it propagates."
                exit 1
            }
        }

        # wix\main.wxs has to name each notice file (WiX has no directory glob
        # here), so the set read from LICENSES\ is checked against it before the
        # MSI is built — a licence text added to the directory but not to the
        # installer fails here instead of shipping an MSI without it.
        $WxsContent = Get-Content (Join-Path "wix" "main.wxs") -Raw
        foreach ($f in Get-NoticeSetFiles) {
            if (-not $WxsContent.Contains("Source='$f'")) {
                Write-Error "wix\main.wxs has no File with Source='$f' (notice set, see THIRD_PARTY_LICENSES.md)"
                exit 1
            }
        }

        if ($cargoWix) {
            $WixArgs = @(
                "wix",
                "--package", "tasty",
                "--profile", $BuildProfile,
                "--no-build",
                "--nocapture",
                "--output", (Join-Path $DistDir "tasty-${Version}-windows-x64.msi")
            )
            cargo @WixArgs
            if ($LASTEXITCODE -ne 0) {
                Write-Error "cargo wix failed with exit code $LASTEXITCODE"
                exit 1
            }
            Write-Host "Installer: $DistDir\tasty-${Version}-windows-x64.msi"
        }
    }
}

Write-Host "==> Verifying artifacts..."
# ZIP: 풀어서 tasty.exe --version
$VerifyDir = Join-Path $env:TEMP "tasty-verify-$([guid]::NewGuid())"
Expand-Archive -Path $ArchivePath -DestinationPath $VerifyDir
$VerifyExe = Join-Path $VerifyDir "tasty.exe"
if (-not (Test-Path $VerifyExe)) {
    Remove-Item -Recurse -Force $VerifyDir -ErrorAction SilentlyContinue
    Write-Error "tasty.exe not found in ZIP"
    exit 1
}
& $VerifyExe --version | Out-Null
$VersionExit = $LASTEXITCODE
Test-NoticeTree $VerifyDir $ArchiveName
Remove-Item -Recurse -Force $VerifyDir -ErrorAction SilentlyContinue
if ($VersionExit -ne 0) {
    Write-Error "tasty.exe --version failed with exit code $VersionExit"
    exit 1
}
# MSI: 권한 필요한 메타 검증은 skip — 파일 존재만 확인
if (-not $SkipMsi -and $BuildProfile -ne "debug") {
    $MsiPath = Join-Path $DistDir "tasty-${Version}-windows-x64.msi"
    if (-not (Test-Path $MsiPath)) {
        Write-Error "MSI missing: $MsiPath"
        exit 1
    }
    # Notice set: an administrative install (msiexec /a) unpacks the MSI's file
    # table into a folder without installing anything or needing elevation. The
    # install tree is found by the inventory file rather than a hard-coded
    # directory name, then compared with the repo copy.
    $MsiExtract = Join-Path $env:TEMP "tasty-msi-$([guid]::NewGuid())"
    $msiProc = Start-Process -FilePath "msiexec.exe" -Wait -PassThru `
        -ArgumentList @("/a", "`"$((Resolve-Path $MsiPath).Path)`"", "/qn", "TARGETDIR=`"$MsiExtract`"")
    if ($msiProc.ExitCode -ne 0) {
        Remove-Item -Recurse -Force $MsiExtract -ErrorAction SilentlyContinue
        Write-Error "msiexec /a failed with exit code $($msiProc.ExitCode)"
        exit 1
    }
    $inventory = @(Get-ChildItem -Path $MsiExtract -Recurse -File -Filter "THIRD_PARTY_LICENSES.md")
    if ($inventory.Count -ne 1) {
        Remove-Item -Recurse -Force $MsiExtract -ErrorAction SilentlyContinue
        Write-Error "expected one THIRD_PARTY_LICENSES.md in the MSI, found $($inventory.Count)"
        exit 1
    }
    Test-NoticeTree $inventory[0].DirectoryName (Split-Path -Leaf $MsiPath)
    Remove-Item -Recurse -Force $MsiExtract -ErrorAction SilentlyContinue
}

$ShaSumsPath = Join-Path $DistDir "SHA256SUMS-windows.txt"
$Sums = @()
$ZipHash = (Get-FileHash $ArchivePath -Algorithm SHA256).Hash.ToLower()
$Sums += "$ZipHash  $(Split-Path -Leaf $ArchivePath)"
if (-not $SkipMsi -and $BuildProfile -ne "debug") {
    $MsiPath = Join-Path $DistDir "tasty-${Version}-windows-x64.msi"
    if (Test-Path $MsiPath) {
        $MsiHash = (Get-FileHash $MsiPath -Algorithm SHA256).Hash.ToLower()
        $Sums += "$MsiHash  $(Split-Path -Leaf $MsiPath)"
    }
}
$Sums | Out-File -Encoding ASCII $ShaSumsPath

Write-Host ""
Write-Host "Done!"
Write-Host "SHA: $ShaSumsPath"

} finally {
    Pop-Location
}
