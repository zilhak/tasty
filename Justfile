# Tasty build & dev task runner.
#
# dist 빌드(OS 자동 감지/정리/사전 도구/SHA 재검증), 플러그인 빌드·스테이징,
# 개발 실행(just run)을 제공한다.
#
# 사용:
#   just build              # 본체+플러그인 debug 빌드·스테이징 (실행 X)
#   just build --release    # 본체+플러그인 release 빌드·스테이징 (실행 X)
#   just run [ARGS]         # 본체+플러그인 debug 빌드 + 호스트 실행 (개발용)
#   just run --release      # 본체+플러그인 release 빌드 + 호스트 실행
#   just install            # 본체+플러그인 dist 빌드 + 현재 머신에 설치 (OS 자동 감지)
#   just build-plugins      # 플러그인 빌드·스테이징
#   just build-all          # main bin + 플러그인
#   just dist               # 호스트 OS 자동 감지 (배포 산출물)
#   just dist-macos         # 플랫폼 명시
#   just dist-linux
#   just dist-windows
#   just dist-clean         # dist/ + cargo-deb / generate-rpm / AppDir 정리
#   just dist-setup-linux   # cargo-deb / generate-rpm / linuxdeploy 자동 설치
#   just dist-verify        # SHA256SUMS 재검증
#
# 사전 조건:
#   - cargo install just  (또는 winget install Casey.Just)
#   - 모든 레시피는 bash 스크립트다. Windows 에서는 Git for Windows 의 bash 가
#     필요하며, just 가 shebang 경로를 변환할 때 cygpath 를 쓰므로
#     `C:\Program Files\Git\usr\bin` 이 PATH 에 있어야 한다(cygpath 위치).
#     이 조건만 충족하면 PowerShell / cmd / Git Bash 어디서 실행해도 동작한다.
#
# 주의: shebang 은 반드시 `#!/bin/bash` (절대경로) 를 쓴다. `#!/usr/bin/env bash`
# 로 하면 Windows 에서 env 가 PATH 를 2차 탐색해 System32 의 WSL bash 를 잡아
# 실패한다. `/bin/bash` 는 cygpath 가 Git bash 로 직접 변환하므로 안전하고,
# macOS(/bin/bash) · Linux 에도 그대로 호환된다.

default:
    @just --list

# 호스트 OS 자동 감지 → 해당 스크립트 실행
# (Windows 는 Git Bash 환경에서만 자동 감지 가능, 일반적으로 just dist-windows 권장)
dist:
    #!/bin/bash
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

# 본체 + 플러그인 전체를 dist 프로필로 빌드해 현재 머신에 설치 (전부 최신본으로 덮어쓰기).
# 호스트 OS 자동 감지. macOS 는 /Applications/Tasty.app 으로 설치하고, 플러그인은 앱 첫
# 실행 시 호스트가 ~/.tasty/plugins 로 강제 덮어쓰기 동기화한다.
install:
    #!/bin/bash
    set -euo pipefail
    case "$(uname -s)" in
        Darwin)
            exec ./scripts/install-macos.sh ;;
        Linux)
            echo "Linux 자동 설치는 아직 미구현입니다." >&2
            echo "  just dist-linux 로 산출물(.deb/.rpm/AppImage)을 빌드한 뒤 수동 설치하세요." >&2
            exit 1 ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "Windows 자동 설치는 아직 미구현입니다." >&2
            echo "  just dist-windows 로 .msi 를 빌드한 뒤 설치하세요." >&2
            exit 1 ;;
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
    #!/bin/bash
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

# ────────────────────────────────────────────────────────────
# Plugin 빌드 / 스테이징
# ────────────────────────────────────────────────────────────
#
# bundle_root() (crates/tasty-host-plugin/src/builtin.rs) 의 fallback 경로
# `<exe_dir>/builtin-plugins/` = `target/<profile>/builtin-plugins/` 에
# plugin 산출물(tasty-plugin.toml + bin + lang/) 을 스테이징한다.
# tasty 부팅 시 `install_builtins_if_needed` 가 거기서 사용자
# `~/.tasty/plugins/<id>/` 로 자동 sync 한다.
#
# 사용:
#   just build-plugins              # 모든 bin plugin → release 스테이징
#   PROFILE=debug just build-plugins  # debug 프로필 (cargo build, target/debug/)
#   just build-plugin claude        # 단일 plugin (이름/crate/manifest id 허용)
#   just build-all                  # plugins + main bin
#   just link-plugins               # cp 대신 symlink (dev 가속용)

# profile 선택 (release 기본; debug 도 가능)
PROFILE := env_var_or_default('PROFILE', 'release')

# 모든 bin plugin crate 를 빌드 + 스테이징.
# 판별 기준: crates/tasty-plugin-* 중 tasty-plugin.toml 보유 = bin plugin.
# manifest 없는 lib-only crate (protocol, sdk, manifest, sdk-wasm) 는 자동 skip.
build-plugins:
    #!/bin/bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    profile_dir="target/$profile"
    bundle_root="$profile_dir/builtin-plugins"

    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) exe_ext=".exe" ;;
        *)                    exe_ext="" ;;
    esac

    crates=()
    for d in crates/tasty-plugin-*; do
        [ -f "$d/tasty-plugin.toml" ] || continue
        crates+=("$(basename "$d")")
    done
    if [ "${#crates[@]}" -eq 0 ]; then
        echo "no plugin crates found under crates/tasty-plugin-*" >&2
        exit 1
    fi

    # 서명 키 보장 + 임베드 pubkey 재도출 — cargo build 전에. release/dist 프로필은
    # trust 게이트가 켜지므로(#[cfg(debug_assertions)] off), dist 스크립트와 동일하게
    # 빌드 시점에 키를 보장해야 builtin 이 자동 trust 된다. host `tasty` 바이너리는
    # build/run/build-all 에서 이 recipe **다음에** 컴파일되므로, 여기서 재도출한
    # dev-pubkey.bin 이 그 빌드에 임베드된다(순서 불변식). debug 는 게이트가 꺼져 있어
    # 건너뛴다(기본 dev 워크플로에 openssl 의존을 부과하지 않음).
    if [ "$profile" != debug ]; then
        SIGN_KEY_PATH="$(bash ./scripts/ensure-sign-key.sh)"
        export SIGN_KEY_PATH
    fi

    # 모든 plugin 을 단일 cargo 호출로 — dep graph 1회 해석.
    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    # release/dist: 모든 builtin 매니페스트를 재서명(--all-builtins). 매니페스트가
    # 바뀌면(버전 자동 bump 포함) 기존 .sig 가 무효화되므로 빌드 시점에 흡수.
    if [ "$profile" != debug ]; then
        bash ./scripts/sign-bundle.sh --key "$SIGN_KEY_PATH" --all-builtins
    fi

    mkdir -p "$bundle_root"
    for c in "${crates[@]}"; do
        d="crates/$c"
        # `|| true` 가 없으면 아래 -z 분기가 죽는다 — grep 이 못 찾았을 때
        # pipefail + set -e 가 대입 자리에서 먼저 죽여 진단이 발화하지 못한다.
        id=$(grep -m1 -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" \
            | sed 's/.*"\([^"]*\)".*/\1/' || true)
        if [ -z "$id" ]; then
            echo "✘ $c: cannot parse id from $d/tasty-plugin.toml" >&2
            exit 1
        fi
        bin_name="$c$exe_ext"
        src_bin="$profile_dir/$bin_name"
        if [ ! -f "$src_bin" ]; then
            echo "✘ $c: built binary missing at $src_bin" >&2
            exit 1
        fi
        dest="$bundle_root/$id"
        mkdir -p "$dest"
        cp "$src_bin" "$dest/$bin_name"
        cp "$d/tasty-plugin.toml" "$dest/tasty-plugin.toml"
        # .sig sidecar — 위 sign-bundle.sh 산출물. non-debug 는 필수(없으면 서명 실패),
        # debug 는 게이트 우회라 선택.
        if [ -f "$d/tasty-plugin.toml.sig" ]; then
            cp "$d/tasty-plugin.toml.sig" "$dest/tasty-plugin.toml.sig"
        elif [ "$profile" != debug ]; then
            echo "✘ $c: missing $d/tasty-plugin.toml.sig (signing failed?)" >&2
            exit 1
        fi
        if [ -d "$d/lang" ]; then
            rm -rf "$dest/lang"
            cp -R "$d/lang" "$dest/lang"
        fi
        echo "✓ staged $id → $dest"
    done

# 단일 plugin build + 스테이징.
# 인자 허용 형태: "claude" / "tasty-plugin-claude" / "com.tasty.claude"
build-plugin name:
    #!/bin/bash
    set -euo pipefail
    name="{{name}}"
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    profile_dir="target/$profile"
    bundle_root="$profile_dir/builtin-plugins"

    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) exe_ext=".exe" ;;
        *)                    exe_ext="" ;;
    esac

    # 정규화: 입력으로 crate 디렉토리를 찾는다.
    crate=""
    plugin_id=""
    for d in crates/tasty-plugin-*; do
        [ -f "$d/tasty-plugin.toml" ] || continue
        c=$(basename "$d")
        id=$(grep -m1 -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" \
            | sed 's/.*"\([^"]*\)".*/\1/')
        short=${c#tasty-plugin-}
        if [ "$name" = "$c" ] || [ "$name" = "$short" ] || [ "$name" = "$id" ]; then
            crate="$c"
            plugin_id="$id"
            break
        fi
    done
    if [ -z "$crate" ]; then
        echo "✘ plugin not found: $name" >&2
        echo "  사용 가능한 이름:" >&2
        for d in crates/tasty-plugin-*; do
            [ -f "$d/tasty-plugin.toml" ] || continue
            c=$(basename "$d")
            short=${c#tasty-plugin-}
            id=$(grep -m1 -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" \
                | sed 's/.*"\([^"]*\)".*/\1/')
            echo "    $short  ($c, $id)" >&2
        done
        exit 1
    fi

    cargo build $profile_flag -p "$crate"
    bin_name="$crate$exe_ext"
    src_bin="$profile_dir/$bin_name"
    if [ ! -f "$src_bin" ]; then
        echo "✘ $crate: built binary missing at $src_bin" >&2
        exit 1
    fi
    dest="$bundle_root/$plugin_id"
    mkdir -p "$dest"
    cp "$src_bin" "$dest/$bin_name"
    cp "crates/$crate/tasty-plugin.toml" "$dest/tasty-plugin.toml"
    if [ -d "crates/$crate/lang" ]; then
        rm -rf "$dest/lang"
        cp -R "crates/$crate/lang" "$dest/lang"
    fi
    echo "✓ staged $plugin_id → $dest"

# main bin + 모든 plugin 한 번에.
build-all: build-plugins
    #!/bin/bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    cargo build $profile_flag --bin tasty

# 풀빌드 — 본체+플러그인 빌드·스테이징 (실행 없음).
#   just build            # debug 빌드 (기본)
#   just build --release  # release 빌드
# run 과 동일하게 플러그인을 빌드·스테이징하고 본 바이너리도 빌드한다. 다만 실행은 하지
# 않으므로, 스테이징본(target/<profile>/builtin-plugins)이 ~/.tasty/plugins 로 강제
# 덮어쓰기되는 건 다음 호스트 실행 시점이다.
build *ARGS:
    #!/bin/bash
    set -euo pipefail
    profile="${PROFILE:-debug}"
    for arg in {{ARGS}}; do
        case "$arg" in
            --release) profile="release" ;;
            --debug)   profile="debug" ;;
            *) echo "build: 알 수 없는 인자 '$arg'" >&2; exit 1 ;;
        esac
    done
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    PROFILE="$profile" just build-plugins
    cargo build $profile_flag --bin tasty

# 개발 실행 — 플러그인 풀빌드 + 호스트 실행.
# build-plugins 로 플러그인을 빌드·스테이징한 뒤 호스트를 실행한다. 호스트는 시작 시
# builtin 을 번들본으로 항상 무조건 덮어쓰기 설치하므로(install_builtins_if_needed),
# 플러그인 소스 변경이 버전 bump 없이도 매 실행 반영된다.
#   just run            # debug 빌드 (기본)
#   just run --release  # release 빌드
# --release 는 ARGS 에서 분리해 프로필로 해석하고, 나머지 인자는 호스트로 passthrough.
run *ARGS:
    #!/bin/bash
    set -euo pipefail
    profile="${PROFILE:-debug}"
    passthrough=()
    for arg in {{ARGS}}; do
        case "$arg" in
            --release) profile="release" ;;
            --debug)   profile="debug" ;;
            *)         passthrough+=("$arg") ;;
        esac
    done
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    PROFILE="$profile" just build-plugins
    # bash 3.2: 빈 배열 "${arr[@]}" 가 set -u 에서 unbound 이므로 분기.
    if [ "${#passthrough[@]}" -gt 0 ]; then
        cargo run $profile_flag --bin tasty -- "${passthrough[@]}"
    else
        cargo run $profile_flag --bin tasty
    fi

# 빌드된 plugin 산출물을 cp 대신 symlink 로 스테이징.
# rebuild 후 별도 sync 단계 없이 새 binary 즉시 반영 — H (auto-reload) 시너지.
# (debug 빌드는 이미 ensure_dev_bundle 이 mtime 기반 자동 sync 하므로
#  주로 release 빌드의 dev 반복 가속용.)
link-plugins:
    #!/bin/bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    profile_dir="target/$profile"
    bundle_root="$profile_dir/builtin-plugins"

    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) exe_ext=".exe" ;;
        *)                    exe_ext="" ;;
    esac

    crates=()
    for d in crates/tasty-plugin-*; do
        [ -f "$d/tasty-plugin.toml" ] || continue
        crates+=("$(basename "$d")")
    done

    # build-plugins 와 동일: release/dist 는 trust 게이트가 켜지므로 키 보장 +
    # pubkey 재도출을 cargo build 전에 수행한다. (debug 는 게이트 우회 → 건너뜀.)
    if [ "$profile" != debug ]; then
        SIGN_KEY_PATH="$(bash ./scripts/ensure-sign-key.sh)"
        export SIGN_KEY_PATH
    fi

    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    # release/dist: crate-dir 매니페스트 재서명. .sig 를 symlink 하므로 이후 재서명이
    # 번들에 자동 반영된다(link-plugins 의 dev 반복 가속 취지와 일치).
    if [ "$profile" != debug ]; then
        bash ./scripts/sign-bundle.sh --key "$SIGN_KEY_PATH" --all-builtins
    fi

    mkdir -p "$bundle_root"
    abs_workspace=$(pwd)
    for c in "${crates[@]}"; do
        d="crates/$c"
        id=$(grep -m1 -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" \
            | sed 's/.*"\([^"]*\)".*/\1/')
        bin_name="$c$exe_ext"
        src_bin="$abs_workspace/$profile_dir/$bin_name"
        if [ ! -f "$src_bin" ]; then
            echo "✘ $c: built binary missing at $src_bin" >&2
            exit 1
        fi
        dest="$bundle_root/$id"
        mkdir -p "$dest"
        ln -sfn "$src_bin" "$dest/$bin_name"
        ln -sfn "$abs_workspace/$d/tasty-plugin.toml" "$dest/tasty-plugin.toml"
        # .sig sidecar — crate-dir 파일을 symlink (재서명 시 자동 반영).
        if [ -f "$d/tasty-plugin.toml.sig" ]; then
            ln -sfn "$abs_workspace/$d/tasty-plugin.toml.sig" "$dest/tasty-plugin.toml.sig"
        elif [ "$profile" != debug ]; then
            echo "✘ $c: missing $d/tasty-plugin.toml.sig (signing failed?)" >&2
            exit 1
        fi
        if [ -d "$d/lang" ]; then
            rm -rf "$dest/lang"
            ln -sfn "$abs_workspace/$d/lang" "$dest/lang"
        fi
        echo "✓ linked $id → $dest"
    done

# SHA256SUMS 재검증.
dist-verify:
    #!/bin/bash
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
