# Tasty dist build wrapper.
#
# 일상 cargo build / test 는 wrapping 하지 않는다 (사용자 권고 Q4).
# dist 빌드의 OS 자동 감지, 정리, 사전 도구 설치, SHA 재검증만 노출한다.
#
# 사용:
#   just dist               # 호스트 OS 자동 감지
#   just dist-macos         # 플랫폼 명시
#   just dist-linux
#   just dist-windows
#   just dist-clean         # dist/ + cargo-deb / generate-rpm / AppDir 정리
#   just dist-setup-linux   # cargo-deb / generate-rpm / linuxdeploy 자동 설치
#   just dist-verify        # SHA256SUMS 재검증
#
# 사전 조건: cargo install just

default:
    @just --list

# 호스트 OS 자동 감지 → 해당 스크립트 실행
# (Windows 는 Git Bash 환경에서만 자동 감지 가능, 일반적으로 just dist-windows 권장)
dist:
    #!/usr/bin/env bash
    set -euo pipefail
    case "$(uname -s)" in
        Darwin)
            exec ./scripts/build-macos-dmg.sh ;;
        Linux)
            exec ./scripts/build-linux.sh ;;
        MINGW*|MSYS*|CYGWIN*)
            exec pwsh -File ./scripts/build-windows.ps1 ;;
        *)
            echo "Unsupported OS: $(uname -s)" >&2
            exit 1 ;;
    esac

dist-macos:
    ./scripts/build-macos-dmg.sh

dist-linux:
    ./scripts/build-linux.sh

dist-windows:
    pwsh -File ./scripts/build-windows.ps1

dist-clean:
    rm -rf dist target/debian target/generate-rpm target/AppDir

# Linux 사전 도구 자동 설치 (cargo-deb, cargo-generate-rpm, linuxdeploy).
# sudo 권한 필요 (apt install 단계).
dist-setup-linux:
    #!/usr/bin/env bash
    set -euo pipefail
    sudo apt install -y cmake pkg-config libfreetype6-dev libfontconfig1-dev
    cargo install cargo-deb cargo-generate-rpm
    if ! command -v linuxdeploy &>/dev/null; then
        mkdir -p "$HOME/.local/bin"
        curl -fsSL -o "$HOME/.local/bin/linuxdeploy" \
            "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage"
        chmod +x "$HOME/.local/bin/linuxdeploy"
        echo "linuxdeploy installed to ~/.local/bin (ensure on PATH)"
    fi

# SHA256SUMS 재검증.
dist-verify:
    #!/usr/bin/env bash
    set -euo pipefail
    cd dist
    case "$(uname -s)" in
        Darwin)
            shasum -a 256 --check SHA256SUMS-macos.txt ;;
        Linux)
            ARCH_RAW=$(uname -m)
            case "$ARCH_RAW" in
                x86_64)  ARCH=x64 ;;
                aarch64) ARCH=arm64 ;;
                *) echo "Unsupported architecture: $ARCH_RAW" >&2; exit 1 ;;
            esac
            sha256sum --check "SHA256SUMS-linux-${ARCH}.txt" ;;
        *)
            echo "On Windows, run: Get-FileHash dist\\*.zip,dist\\*.msi -Algorithm SHA256" >&2
            exit 1 ;;
    esac
