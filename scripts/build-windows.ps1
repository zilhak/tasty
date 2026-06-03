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
#   dist\tasty-{version}-windows-x64.msi   (installer, requires WiX 3.x)

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

Write-Host "==> Building tasty ($BuildProfile)..."
cargo build @CargoFlags
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed with exit code $LASTEXITCODE"
    exit 1
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

Write-Host "==> Creating $ArchiveName..."
$ArchivePath = Join-Path $DistDir $ArchiveName
if (Test-Path $ArchivePath) { Remove-Item -Force $ArchivePath }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ArchivePath

Remove-Item -Recurse -Force $StageDir

Write-Host ""
Write-Host "Portable archive: $DistDir\$ArchiveName"

# === MSI installer (cargo-wix) ===
if (-not $SkipMsi) {
    Write-Host ""
    Write-Host "==> Building MSI installer..."

    # cargo-wix는 release/dist 프로필만 지원 (debug는 의미 없음)
    if ($BuildProfile -eq "debug") {
        Write-Host "  (skipping MSI for debug build)"
    } else {
        $cargoWix = Get-Command cargo-wix -ErrorAction SilentlyContinue
        if (-not $cargoWix) {
            Write-Warning "cargo-wix not found. Install with: cargo install cargo-wix"
            Write-Warning "Skipping MSI build."
        } else {
            # WiX 3.x (winget package WiXToolset.WiXToolset) registers the WIX
            # environment variable but does NOT add WIX\bin to PATH, so cargo-wix
            # can't find candle.exe / light.exe. Prepend it for this process.
            if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
                $wixRoot = $env:WIX
                if (-not $wixRoot) {
                    $wixRoot = [Environment]::GetEnvironmentVariable("WIX", "Machine")
                }
                $wixBin = if ($wixRoot) { Join-Path $wixRoot "bin" } else { $null }
                if ($wixBin -and (Test-Path (Join-Path $wixBin "candle.exe"))) {
                    $env:Path = "$wixBin;$env:Path"
                    Write-Host "  (added $wixBin to PATH for cargo-wix)"
                } else {
                    Write-Warning "WiX Toolset not found. Install with: winget install WiXToolset.WiXToolset"
                    Write-Warning "Skipping MSI build."
                    $cargoWix = $null
                }
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
}

Write-Host ""
Write-Host "Done!"

} finally {
    Pop-Location
}
