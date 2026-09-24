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

# $Profile은 PowerShell 자동 변수이므로 빌드 프로필에는 $BuildProfile을 사용한다.
$BuildProfile = "dist"
$CargoFlags = @("--profile", "dist")
if ($Debug) {
    $BuildProfile = "debug"
    $CargoFlags = @()
} elseif ($Release) {
    $BuildProfile = "release"
    $CargoFlags = @("--release")
}

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

# WSL bash가 선택되는 일을 줄이기 위해 Git Bash를 우선 찾는다.
function Resolve-Bash {
    $candidates = @()
    $gitCmd = Get-Command git -ErrorAction SilentlyContinue
    if ($gitCmd) {
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
    $b = Get-Command bash -ErrorAction SilentlyContinue
    if ($b) { return $b.Source }
    return $null
}

# 빌드 전에 서명 키를 준비해 대응 공개키가 바이너리에 포함되게 한다.
if ($BuildProfile -ne "debug") {
    $SignKeyPath = $env:SIGN_KEY_PATH
    if (-not $SignKeyPath) {
        $ReleaseKey = Join-Path $env:USERPROFILE ".tasty-keys\release.pem"
        $DevKey = Join-Path $env:USERPROFILE ".tasty-keys\dev.pem"
        if (Test-Path $ReleaseKey) {
            $SignKeyPath = $ReleaseKey
        } else {
            # 개발 키가 없으면 만들고 기존 키가 있으면 공개키를 다시 추출한다.
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

# 매니페스트가 있고 bundle=false가 아닌 plugin을 배포에 포함한다.
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

# notice 파일은 LICENSE·THIRD_PARTY_LICENSES.md·LICENSES 바로 아래 파일이다. Linux/macOS와 같은 목록을 사용한다.
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

# notice 문서의 상대 링크가 맞도록 LICENSES 디렉터리를 유지한다.
function Stage-Notice([string]$Dest) {
    New-Item -ItemType Directory -Force -Path (Join-Path $Dest "LICENSES") | Out-Null
    foreach ($f in Get-NoticeSetFiles) {
        Copy-Item $f -Destination (Join-Path $Dest $f)
    }
}

# SHA256으로 배포 사본을 원본과 대조한다.
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

$BuildDir = Join-Path "target" $BuildProfile
Get-ChildItem -Path $BuildDir -Filter "*.dll" | ForEach-Object {
    Copy-Item $_.FullName -Destination $StageDir
}

Stage-Plugins (Join-Path $StageDir "plugins")
Stage-Notice $StageDir

Write-Host "==> Creating $ArchiveName..."
$ArchivePath = Join-Path $DistDir $ArchiveName
if (Test-Path $ArchivePath) { Remove-Item -Force $ArchivePath }
Compress-Archive -Path (Join-Path $StageDir "*") -DestinationPath $ArchivePath

Remove-Item -Recurse -Force $StageDir

Write-Host ""
Write-Host "Portable archive: $DistDir\$ArchiveName"

# MSI의 plugin 파일 목록은 wix/main.wxs가 별도로 관리한다.
if (-not $SkipMsi) {
    Write-Host ""
    Write-Host "==> Building MSI installer..."

    if ($BuildProfile -eq "debug") {
        Write-Host "  (skipping MSI for debug build)"
    } else {
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

        # cargo-wix가 candle/light를 찾도록 WiX 설치 위치의 bin을 PATH에 추가한다.
        if (-not (Get-Command candle.exe -ErrorAction SilentlyContinue)) {
            $wixRoot = $env:WIX
            if (-not $wixRoot) {
                $wixRoot = [Environment]::GetEnvironmentVariable("WIX", "Machine")
            }
            $wixBin = if ($wixRoot) { Join-Path $wixRoot "bin" } else { $null }

            # WiX 설치는 관리자 자식 PowerShell에서 요청하며 UAC 승인이 필요할 수 있다.
            if (-not ($wixBin -and (Test-Path (Join-Path $wixBin "candle.exe")))) {
                Write-Host "==> WiX Toolset not found — installing via winget (a UAC admin prompt will appear)..."
                $winget = Get-Command winget -ErrorAction SilentlyContinue
                if (-not $winget) {
                    Write-Error "winget not found. Install WiX 3.x manually: winget install -e --id WiXToolset.WiXToolset"
                    exit 1
                }
                $elevCmd = 'Enable-WindowsOptionalFeature -Online -FeatureName NetFx3 -All -NoRestart -ErrorAction SilentlyContinue | Out-Null; winget install -e --id WiXToolset.WiXToolset --accept-source-agreements --accept-package-agreements; exit $LASTEXITCODE'
                try {
                    $proc = Start-Process -FilePath "powershell.exe" -Verb RunAs -Wait -PassThru `
                        -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-Command', $elevCmd)
                } catch {
                    Write-Error "Could not elevate to install WiX (UAC declined or unavailable): $($_.Exception.Message). Install manually: winget install -e --id WiXToolset.WiXToolset"
                    exit 1
                }
                # 설치 종료 코드만으로 중단하지 않고 아래에서 candle.exe를 다시 확인한다.
                if ($proc.ExitCode -ne 0) {
                    Write-Warning "Elevated WiX install returned exit code $($proc.ExitCode) — verifying candle.exe anyway..."
                }
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

        # WiX는 파일을 명시하므로 notice 목록의 Source 항목이 선언됐는지 먼저 확인한다.
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
if (-not $SkipMsi -and $BuildProfile -ne "debug") {
    $MsiPath = Join-Path $DistDir "tasty-${Version}-windows-x64.msi"
    if (-not (Test-Path $MsiPath)) {
        Write-Error "MSI missing: $MsiPath"
        exit 1
    }
    # MSI를 별도 디렉터리에 풀어 notice 파일을 원본과 대조한다.
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
