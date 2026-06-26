# Tasty build & dev task runner.
#
# dist 빌드(OS 자동 감지/정리/사전 도구/SHA 재검증), 플러그인 빌드·스테이징,
# 개발 실행(just run)을 제공한다.
#
# 사용:
#   just run [ARGS]         # 플러그인(debug) 빌드 + 호스트 실행 (개발용)
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
    #!/usr/bin/env bash
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

    # 모든 plugin 을 단일 cargo 호출로 — dep graph 1회 해석.
    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    mkdir -p "$bundle_root"
    for c in "${crates[@]}"; do
        d="crates/$c"
        id=$(grep -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" | head -1 \
            | sed 's/.*"\([^"]*\)".*/\1/')
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
        if [ -d "$d/lang" ]; then
            rm -rf "$dest/lang"
            cp -R "$d/lang" "$dest/lang"
        fi
        echo "✓ staged $id → $dest"
    done

# 단일 plugin build + 스테이징.
# 인자 허용 형태: "claude" / "tasty-plugin-claude" / "com.tasty.claude"
build-plugin name:
    #!/usr/bin/env bash
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
        id=$(grep -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" | head -1 \
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
            id=$(grep -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" | head -1 \
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
    #!/usr/bin/env bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    cargo build $profile_flag --bin tasty

# 개발 실행 — 플러그인 풀빌드 + 호스트 실행.
# build-plugins 로 플러그인을 빌드·스테이징한 뒤 호스트를 실행한다. 호스트는 시작 시
# builtin 을 번들본으로 항상 무조건 덮어쓰기 설치하므로(install_builtins_if_needed),
# 플러그인 소스 변경이 버전 bump 없이도 매 실행 반영된다. PROFILE 은 debug 기본
# (cargo run 과 경로 일치) — 릴리즈는 `PROFILE=release just run`.
run *ARGS:
    #!/usr/bin/env bash
    set -euo pipefail
    profile="${PROFILE:-debug}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    PROFILE="$profile" just build-plugins
    cargo run $profile_flag --bin tasty {{ARGS}}

# 빌드된 plugin 산출물을 cp 대신 symlink 로 스테이징.
# rebuild 후 별도 sync 단계 없이 새 binary 즉시 반영 — H (auto-reload) 시너지.
# (debug 빌드는 이미 ensure_dev_bundle 이 mtime 기반 자동 sync 하므로
#  주로 release 빌드의 dev 반복 가속용.)
link-plugins:
    #!/usr/bin/env bash
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

    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    mkdir -p "$bundle_root"
    abs_workspace=$(pwd)
    for c in "${crates[@]}"; do
        d="crates/$c"
        id=$(grep -E '^id[[:space:]]*=' "$d/tasty-plugin.toml" | head -1 \
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
        if [ -d "$d/lang" ]; then
            rm -rf "$dest/lang"
            ln -sfn "$abs_workspace/$d/lang" "$dest/lang"
        fi
        echo "✓ linked $id → $dest"
    done

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
