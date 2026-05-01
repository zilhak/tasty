# Build a Windows ZIP archive for Tasty.
#
# Usage:
#   .\scripts\build-windows.ps1           # dist build (full LTO, 배포용)
#   .\scripts\build-windows.ps1 -Release  # release build (thin LTO, 빠른 빌드)
#   .\scripts\build-windows.ps1 -Debug    # debug build
#
# Output:
#   dist\tasty-{version}-windows-x64.zip

param(
    [switch]$Release,
    [switch]$Debug
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
$Profile = "dist"
$CargoFlags = @("--profile", "dist")
if ($Debug) {
    $Profile = "debug"
    $CargoFlags = @()
} elseif ($Release) {
    $Profile = "release"
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

Write-Host "==> Building tasty ($Profile)..."
cargo build @CargoFlags
if ($LASTEXITCODE -ne 0) {
    Write-Error "cargo build failed with exit code $LASTEXITCODE"
    exit 1
}

Write-Host "==> Assembling archive..."
if (Test-Path $StageDir) { Remove-Item -Recurse -Force $StageDir }
New-Item -ItemType Directory -Force -Path $StageDir | Out-Null

$ExePath = Join-Path (Join-Path "target" $Profile) "tasty.exe"
if (-not (Test-Path $ExePath)) {
    Write-Error "Build output not found: $ExePath"
    exit 1
}
Copy-Item $ExePath -Destination $StageDir

# Collect any DLLs from the build output directory
$BuildDir = Join-Path "target" $Profile
Get-ChildItem -Path $BuildDir -Filter "*.dll" | ForEach-Object {
    Copy-Item $_.FullName -Destination $StageDir
}

Write-Host "==> Creating $ArchiveName..."
$ArchivePath = Join-Path $DistDir $ArchiveName
if (Test-Path $ArchivePath) { Remove-Item -Force $ArchivePath }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ArchivePath

Remove-Item -Recurse -Force $StageDir

Write-Host ""
Write-Host "Done!"
Write-Host "  Archive: $DistDir\$ArchiveName"

} finally {
    Pop-Location
}
