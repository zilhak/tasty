# 빌드·플러그인 준비·개발 실행·배포 명령. 전체 목록은 just --list로 확인한다.
# build/run은 기본 debug, build-plugins/build-all은 기본 release를 사용한다.
# build/run에 --release를 주거나 PROFILE 환경변수로 프로필을 선택할 수 있다.
# install은 현재 macOS만 지원한다.
#
# Windows의 Bash 레시피에는 Git for Windows와 cygpath가 필요하다.
# C:\Program Files\Git\usr\bin을 PATH에 포함한다.
# shebang은 /bin/bash를 사용한다. /usr/bin/env bash는 WSL의 bash를 찾을 수 있다.

# 사용 가능한 명령을 표시한다.
default:
    @just --list

# 호스트 OS에 맞는 배포 패키지를 빌드한다.
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

# 플러그인은 호스트 시작 시 번들 설치 정책에 따라 동기화한다.

# macOS 앱 번들을 빌드해 /Applications에 설치한다.
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

# macOS 배포 패키지를 빌드한다.
dist-macos:
    ./scripts/build-macos-dmg.sh

# Linux 배포 패키지를 빌드한다.
dist-linux:
    ./scripts/build-linux.sh

# Windows 배포 패키지를 빌드한다.
dist-windows:
    pwsh -File ./scripts/build-windows.ps1

# 배포 결과와 패키징 중간 파일을 삭제한다.
dist-clean:
    rm -rf dist target/debian target/generate-rpm target/AppDir

# Linux 패키징 도구를 설치한다. apt 단계에는 sudo 권한이 필요하다.
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

# 플러그인·build-all 명령의 기본 프로필. build/run은 별도로 debug를 기본값으로 쓴다.
PROFILE := env_var_or_default('PROFILE', 'release')

# TASTY_E2E_BIN을 허용하는 테스트에서만 사용할 수 있다.
# 사용법: BIN=$(just e2e-headless-bin) && export TASTY_E2E_BIN=$BIN

# e2e용 headless 바이너리를 빌드하고 경로를 출력한다.
e2e-headless-bin:
    @scripts/build-e2e-headless.sh

# 매니페스트가 있는 플러그인을 빌드해 실행 파일 옆 builtin-plugins에 준비한다.
build-plugins:
    #!/bin/bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    profile_dir="${CARGO_TARGET_DIR:-target}/$profile"
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

    # 본체에 공개키를 포함할 수 있도록 non-debug 빌드 전에 서명 키를 준비한다.
    if [ "$profile" != debug ]; then
        SIGN_KEY_PATH="$(bash ./scripts/ensure-sign-key.sh)"
        export SIGN_KEY_PATH
    fi

    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    # 매니페스트가 바뀌면 기존 서명은 유효하지 않으므로 다시 서명한다.
    if [ "$profile" != debug ]; then
        bash ./scripts/sign-bundle.sh --key "$SIGN_KEY_PATH" --all-builtins
    fi

    mkdir -p "$bundle_root"
    for c in "${crates[@]}"; do
        d="crates/$c"
        # grep 실패를 뒤의 빈 ID 진단에서 처리하도록 || true를 둔다.
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

# 지정한 플러그인을 빌드하고 번들 폴더에 복사한다. 이름·크레이트명·매니페스트 ID를 받는다.
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
    profile_dir="${CARGO_TARGET_DIR:-target}/$profile"
    bundle_root="$profile_dir/builtin-plugins"

    case "$(uname -s)" in
        MINGW*|MSYS*|CYGWIN*) exe_ext=".exe" ;;
        *)                    exe_ext="" ;;
    esac

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

# 플러그인을 준비한 뒤 본체를 빌드한다.
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

# 사용자 설치 폴더에는 다음 호스트 시작 때 번들 정책에 따라 반영한다.

# 본체·플러그인을 빌드하고 번들을 준비한다. 호스트는 실행하지 않는다.
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

# --release/--debug는 프로필로 해석하고 나머지 인자는 호스트에 전달한다.
# 호스트는 같은 버전의 변경 파일을 동기화하며 더 높은 설치 버전은 유지한다.

# 본체·플러그인을 빌드하고 호스트를 실행한다.
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
    # Bash 3.2의 set -u에서 빈 배열을 펼치면 오류가 나므로 분기한다.
    if [ "${#passthrough[@]}" -gt 0 ]; then
        cargo run $profile_flag --bin tasty -- "${passthrough[@]}"
    else
        cargo run $profile_flag --bin tasty
    fi

# 번들 링크 갱신과 실행 중인 플러그인의 재시작은 별개다.

# 플러그인을 빌드하고 복사 대신 심볼릭 링크로 번들을 준비한다.
link-plugins:
    #!/bin/bash
    set -euo pipefail
    profile="{{PROFILE}}"
    case "$profile" in
        release) profile_flag="--release" ;;
        debug)   profile_flag="" ;;
        *)       profile_flag="--profile $profile" ;;
    esac
    profile_dir="${CARGO_TARGET_DIR:-target}/$profile"
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

    # 본체에 공개키를 포함할 수 있도록 non-debug 빌드 전에 서명 키를 준비한다.
    if [ "$profile" != debug ]; then
        SIGN_KEY_PATH="$(bash ./scripts/ensure-sign-key.sh)"
        export SIGN_KEY_PATH
    fi

    cargo_args=()
    for c in "${crates[@]}"; do
        cargo_args+=("-p" "$c")
    done
    cargo build $profile_flag "${cargo_args[@]}"

    # 매니페스트가 바뀌면 기존 서명은 유효하지 않으므로 다시 서명한다.
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

# 배포 파일의 SHA256SUMS를 다시 확인한다.
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
