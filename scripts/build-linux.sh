#!/usr/bin/env bash
# Build Linux distribution artifacts for Tasty.
#
# Usage:
#   ./scripts/build-linux.sh           # dist build (full LTO, 배포용)
#   ./scripts/build-linux.sh --release # release build (thin LTO, 빠른 빌드)
#   ./scripts/build-linux.sh --debug   # debug build (tar.gz만)
#
# Output:
#   dist/tasty-{version}-linux-{arch}.tar.gz       # arch: x64 | arm64
#   dist/tasty_{version}-1_{deb-arch}.deb          # debug 제외, deb-arch: amd64 | arm64
#   dist/tasty-{version}-1.{rpm-arch}.rpm          # debug 제외, rpm-arch: x86_64 | aarch64
#   dist/Tasty-{version}-{rpm-arch}.AppImage       # debug 제외, distro-무관 단일 파일
#
# Requires:
#   cargo install cargo-deb            # .deb 패키지 생성 (debug 빌드에선 생략)
#   cargo install cargo-generate-rpm   # .rpm 패키지 생성 (debug 빌드에선 생략)
#   linuxdeploy (PATH에 위치)          # .AppImage 생성. 다음 한 번만 실행:
#     curl -fsSL -o ~/.local/bin/linuxdeploy \
#       https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-$(uname -m).AppImage
#     chmod +x ~/.local/bin/linuxdeploy

set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
    echo "Error: This script must be run on Linux." >&2
    exit 1
fi

cd "$(dirname "$0")/.."

# Parse arguments
PROFILE="dist"
CARGO_FLAGS="--profile dist"
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE="debug"
    CARGO_FLAGS=""
elif [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

ARCH_RAW=$(uname -m)
case "$ARCH_RAW" in
    x86_64)  ARCH=x64 ;;
    aarch64) ARCH=arm64 ;;
    *) echo "Error: Unsupported architecture: $ARCH_RAW" >&2; exit 1 ;;
esac

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST_DIR="dist"
PKG_DIR="tasty-linux-${ARCH}"
ARCHIVE_NAME="tasty-${VERSION}-linux-${ARCH}.tar.gz"

# Check build dependencies
echo "==> Checking build dependencies..."
MISSING_DEPS=()
for dep in cmake pkg-config; do
    if ! command -v "$dep" &>/dev/null; then
        MISSING_DEPS+=("$dep")
    fi
done
for lib in freetype2 fontconfig; do
    if ! pkg-config --exists "$lib" 2>/dev/null; then
        MISSING_DEPS+=("$lib")
    fi
done
# libxdo (tray-icon → muda) has no reliable .pc file and ships only the runtime
# .so.N without the dev symlink unless the dev package is installed. Probe the
# linker directly so this works on any arch (the -L search path differs per
# arch, but `cc -lxdo` resolves it the same way the real build link step does).
if ! echo 'int main(void){return 0;}' | cc -xc - -lxdo -o /dev/null 2>/dev/null; then
    MISSING_DEPS+=("libxdo")
fi

if [[ ${#MISSING_DEPS[@]} -gt 0 ]]; then
    # Detect the distro package manager. Package names differ across distros
    # (e.g. libxdo-dev / libxdo-devel / xdotool), so map each dep per manager.
    PKG_MGR=""
    for m in apt-get dnf pacman zypper; do
        if command -v "$m" &>/dev/null; then PKG_MGR="$m"; break; fi
    done
    pkg_name_for() {
        case "$PKG_MGR:$1" in
            *:cmake) echo cmake ;;
            apt-get:pkg-config|zypper:pkg-config) echo pkg-config ;;
            dnf:pkg-config) echo pkgconf-pkg-config ;;
            pacman:pkg-config) echo pkgconf ;;
            apt-get:freetype2) echo libfreetype6-dev ;;
            dnf:freetype2)     echo freetype-devel ;;
            pacman:freetype2)  echo freetype2 ;;
            zypper:freetype2)  echo freetype2-devel ;;
            apt-get:fontconfig) echo libfontconfig1-dev ;;
            dnf:fontconfig)     echo fontconfig-devel ;;
            pacman:fontconfig)  echo fontconfig ;;
            zypper:fontconfig)  echo fontconfig-devel ;;
            apt-get:libxdo)        echo libxdo-dev ;;
            dnf:libxdo|zypper:libxdo) echo libxdo-devel ;;
            pacman:libxdo)         echo xdotool ;;
            *) echo "" ;;
        esac
    }

    PKGS=()
    UNMAPPED=()
    for key in "${MISSING_DEPS[@]}"; do
        name="$(pkg_name_for "$key")"
        if [[ -n "$name" ]]; then PKGS+=("$name"); else UNMAPPED+=("$key"); fi
    done

    echo "Error: Missing build dependencies: ${MISSING_DEPS[*]}" >&2

    # Resolve privilege escalation (root needs none; non-root needs sudo).
    SUDO=""
    if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
        command -v sudo &>/dev/null && SUDO="sudo"
    fi

    # Build the install command for the detected manager.
    INSTALL_ARGS=()
    case "$PKG_MGR" in
        apt-get) INSTALL_ARGS=(apt-get install -y "${PKGS[@]}") ;;
        dnf)     INSTALL_ARGS=(dnf install -y "${PKGS[@]}") ;;
        pacman)  INSTALL_ARGS=(pacman -S --noconfirm "${PKGS[@]}") ;;
        zypper)  INSTALL_ARGS=(zypper install -y "${PKGS[@]}") ;;
    esac
    SHOW="${SUDO:+sudo }${INSTALL_ARGS[*]}"

    # Offer to install only with a known manager, a fully mapped package set, an
    # interactive TTY, and a usable privilege path. Otherwise (CI / unknown
    # distro / no sudo) print a manual hint and exit.
    if [[ -n "$PKG_MGR" && ${#UNMAPPED[@]} -eq 0 && -t 0 \
          && ( -n "$SUDO" || "${EUID:-$(id -u)}" -eq 0 ) ]]; then
        printf "  Install now? [%s] [y/N] " "$SHOW" >&2
        read -r reply || reply=""
        if [[ "$reply" =~ ^[Yy]([Ee][Ss])?$ ]]; then
            echo "==> $SHOW"
            if [[ -n "$SUDO" ]]; then sudo "${INSTALL_ARGS[@]}"; else "${INSTALL_ARGS[@]}"; fi \
                || { echo "Error: dependency install failed." >&2; exit 1; }
        else
            echo "  Aborted. Run: $SHOW" >&2
            exit 1
        fi
    else
        echo "  Install with your package manager, e.g.: sudo apt install ${PKGS[*]:-${MISSING_DEPS[*]}}" >&2
        [[ ${#UNMAPPED[@]} -gt 0 ]] && echo "  (no package mapping for: ${UNMAPPED[*]})" >&2
        exit 1
    fi
fi

# Discover signing key BEFORE cargo build — so that any newly generated
# dev-pubkey.bin is embedded into the host-plugin binary at compile time.
if [[ "$PROFILE" != "debug" ]]; then
    if [[ -z "${SIGN_KEY_PATH:-}" ]]; then
        if [[ -f "$HOME/.tasty-keys/release.pem" ]]; then
            SIGN_KEY_PATH="$HOME/.tasty-keys/release.pem"
        elif [[ -f "$HOME/.tasty-keys/dev.pem" ]]; then
            SIGN_KEY_PATH="$HOME/.tasty-keys/dev.pem"
        else
            echo "==> No signing key found — auto-generating dev key for zero-touch build..."
            ./scripts/gen-dev-key.sh
            SIGN_KEY_PATH="$HOME/.tasty-keys/dev.pem"
        fi
    fi
    export SIGN_KEY_PATH
fi

echo "==> Building tasty ($PROFILE)..."
cargo build $CARGO_FLAGS

# Discover bundled plugin crates (any `crates/tasty-plugin-*` with a manifest).
# Matches justfile `build-plugins` recipe and build-macos-dmg.sh — keep in sync.
PLUGIN_CRATES=()
for d in crates/tasty-plugin-*; do
    [ -f "$d/tasty-plugin.toml" ] || continue
    PLUGIN_CRATES+=("$(basename "$d")")
done

if [[ ${#PLUGIN_CRATES[@]} -eq 0 ]]; then
    echo "Error: no plugin crates with tasty-plugin.toml found under crates/" >&2
    exit 1
fi

echo "==> Building ${#PLUGIN_CRATES[@]} plugins ($PROFILE)..."
PLUGIN_CARGO_ARGS=()
for c in "${PLUGIN_CRATES[@]}"; do
    PLUGIN_CARGO_ARGS+=("-p" "$c")
done
cargo build $CARGO_FLAGS "${PLUGIN_CARGO_ARGS[@]}"

# release/dist builds: sign all plugin manifests (Ed25519) with the key
# discovered (or auto-generated) before cargo build.
if [[ "$PROFILE" != "debug" ]]; then
    echo "==> Signing plugin manifests with $SIGN_KEY_PATH..."
    ./scripts/sign-bundle.sh --key "$SIGN_KEY_PATH" --all-builtins
fi

# Stage plugins under <dest>/<id>/. Mirrors macOS build-macos-dmg.sh staging.
# `bundle_root()` (crates/tasty-host-plugin/src/builtin.rs) discovers
# `<exe_dir>/plugins/` and syncs each `<plugin-id>/` into `~/.tasty/plugins/<id>/`
# on first launch.
stage_plugins() {
    local plugins_dir="$1"
    mkdir -p "$plugins_dir"
    for c in "${PLUGIN_CRATES[@]}"; do
        local manifest="crates/$c/tasty-plugin.toml"
        local id
        id=$(grep -E '^id[[:space:]]*=' "$manifest" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
        if [[ -z "$id" ]]; then
            echo "Error: cannot parse id from $manifest" >&2
            exit 1
        fi
        local src_bin="target/$PROFILE/$c"
        if [[ ! -f "$src_bin" ]]; then
            echo "Error: plugin binary missing: $src_bin" >&2
            exit 1
        fi
        local dest="$plugins_dir/$id"
        mkdir -p "$dest"
        cp "$src_bin" "$dest/$c"
        cp "$manifest" "$dest/tasty-plugin.toml"
        # .sig sidecar — produced by sign-bundle.sh above; required for non-debug
        # builds, optional otherwise (debug runtime warns instead of rejecting).
        if [[ -f "crates/$c/tasty-plugin.toml.sig" ]]; then
            cp "crates/$c/tasty-plugin.toml.sig" "$dest/tasty-plugin.toml.sig"
        elif [[ "$PROFILE" != "debug" ]]; then
            echo "Error: missing crates/$c/tasty-plugin.toml.sig (signing failed?)" >&2
            exit 1
        fi
        if [[ -d "crates/$c/lang" ]]; then
            rm -rf "$dest/lang"
            cp -R "crates/$c/lang" "$dest/lang"
        fi
        echo "  staged $id"
    done
}

echo "==> Assembling archive..."
rm -rf "$DIST_DIR/$PKG_DIR"
mkdir -p "$DIST_DIR/$PKG_DIR"

cp "target/$PROFILE/tasty" "$DIST_DIR/$PKG_DIR/tasty"
stage_plugins "$DIST_DIR/$PKG_DIR/plugins"

echo "==> Creating $ARCHIVE_NAME..."
rm -f "$DIST_DIR/$ARCHIVE_NAME"
tar -czf "$DIST_DIR/$ARCHIVE_NAME" -C "$DIST_DIR" "$PKG_DIR"

rm -rf "$DIST_DIR/$PKG_DIR"

DEB_FILE=""
RPM_FILE=""
APPIMAGE_FILE=""
if [[ "$PROFILE" != "debug" ]]; then
    if ! command -v cargo-deb &>/dev/null; then
        echo "Error: cargo-deb not found. Install with: cargo install cargo-deb" >&2
        exit 1
    fi
    if ! command -v cargo-generate-rpm &>/dev/null; then
        echo "Error: cargo-generate-rpm not found. Install with: cargo install cargo-generate-rpm" >&2
        exit 1
    fi
    if ! command -v linuxdeploy &>/dev/null; then
        echo "Error: linuxdeploy not found in PATH. See header of this script for install instructions." >&2
        exit 1
    fi

    # .deb and .rpm ship plugins via Cargo.toml metadata assets
    # (`[package.metadata.deb]` and `[package.metadata.generate-rpm]`).
    # Plugins land in /usr/lib/tasty/plugins/<id>/ — the runtime
    # `bundle_root()` (crates/tasty-host-plugin/src/builtin.rs) picks this up
    # as the linux FHS fallback after the exe-relative `plugins/` lookup
    # fails. cargo-deb / cargo-generate-rpm read those metadata blocks before
    # this script runs `--no-build`, so the binaries must exist in
    # `target/<profile>/` (built above).

    echo "==> Building .deb package..."
    cargo deb --no-build --profile "$PROFILE"
    DEB_SRC=$(ls -t target/debian/tasty_*.deb | head -1)
    cp "$DEB_SRC" "$DIST_DIR/"
    DEB_FILE="$DIST_DIR/$(basename "$DEB_SRC")"

    echo "==> Building .rpm package..."
    cargo generate-rpm --profile "$PROFILE"
    RPM_SRC=$(ls -t target/generate-rpm/tasty-*.rpm | head -1)
    cp "$RPM_SRC" "$DIST_DIR/"
    RPM_FILE="$DIST_DIR/$(basename "$RPM_SRC")"

    echo "==> Building .AppImage..."
    APPIMAGE_NAME="Tasty-${VERSION}-${ARCH_RAW}.AppImage"
    APPDIR="target/AppDir"
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR/usr/bin"
    # linuxdeploy doesn't clean AppDir — pre-stage plugins next to where it will
    # place tasty (AppDir/usr/bin/), so current_exe() sees `<exe_dir>/plugins/`.
    stage_plugins "$APPDIR/usr/bin/plugins"
    VERSION="$VERSION" OUTPUT="$APPIMAGE_NAME" linuxdeploy \
        --appdir "$APPDIR" \
        --executable "target/$PROFILE/tasty" \
        --desktop-file assets/linux/tasty.desktop \
        --icon-file assets/icons/icon_256.png \
        --icon-filename tasty \
        --output appimage
    mv "$APPIMAGE_NAME" "$DIST_DIR/"
    APPIMAGE_FILE="$DIST_DIR/$APPIMAGE_NAME"
fi

echo "==> Verifying artifacts..."
# tar.gz: 내부에 tasty 존재 + 풀어서 --version 호출 가능
tar -tzf "$DIST_DIR/$ARCHIVE_NAME" | grep -q "$PKG_DIR/tasty" || {
    echo "Error: tasty not in $ARCHIVE_NAME" >&2
    exit 1
}
VERIFY_TMP=$(mktemp -d)
tar -xzf "$DIST_DIR/$ARCHIVE_NAME" -C "$VERIFY_TMP"
"$VERIFY_TMP/$PKG_DIR/tasty" --version >/dev/null || {
    rm -rf "$VERIFY_TMP"
    echo "Error: tasty --version failed (from tar.gz)" >&2
    exit 1
}
rm -rf "$VERIFY_TMP"
if [[ -n "$DEB_FILE" ]]; then
    dpkg-deb -I "$DEB_FILE" >/dev/null || {
        echo "Error: dpkg-deb -I failed on $DEB_FILE" >&2
        exit 1
    }
fi
if [[ -n "$RPM_FILE" ]] && command -v rpm &>/dev/null; then
    rpm -qpi "$RPM_FILE" >/dev/null 2>&1 || {
        echo "Error: rpm -qpi failed on $RPM_FILE" >&2
        exit 1
    }
fi
# AppImage: GUI 초기화 hang 회피 — 실행 없이 파일 존재 + ELF 헤더만 확인
if [[ -n "$APPIMAGE_FILE" ]]; then
    [[ -f "$APPIMAGE_FILE" ]] || {
        echo "Error: AppImage missing: $APPIMAGE_FILE" >&2
        exit 1
    }
    file "$APPIMAGE_FILE" | grep -q "ELF" || {
        echo "Error: AppImage is not an ELF binary: $APPIMAGE_FILE" >&2
        exit 1
    }
fi

SHASUMS_FILE="SHA256SUMS-linux-${ARCH}.txt"
(
    cd "$DIST_DIR"
    {
        sha256sum "$ARCHIVE_NAME"
        [[ -n "$DEB_FILE" ]]      && sha256sum "$(basename "$DEB_FILE")"
        [[ -n "$RPM_FILE" ]]      && sha256sum "$(basename "$RPM_FILE")"
        [[ -n "$APPIMAGE_FILE" ]] && sha256sum "$(basename "$APPIMAGE_FILE")"
    } > "$SHASUMS_FILE"
)

echo ""
echo "Done!"
echo "  Archive:  $DIST_DIR/$ARCHIVE_NAME"
[[ -n "$DEB_FILE" ]]      && echo "  Deb:      $DEB_FILE"
[[ -n "$RPM_FILE" ]]      && echo "  Rpm:      $RPM_FILE"
[[ -n "$APPIMAGE_FILE" ]] && echo "  AppImage: $APPIMAGE_FILE"
echo "  SHA:      $DIST_DIR/$SHASUMS_FILE"
