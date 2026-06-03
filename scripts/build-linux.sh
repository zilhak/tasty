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
if [[ ${#MISSING_DEPS[@]} -gt 0 ]]; then
    echo "Error: Missing build dependencies: ${MISSING_DEPS[*]}" >&2
    echo "  Install with: sudo apt install cmake pkg-config libfreetype6-dev libfontconfig1-dev" >&2
    exit 1
fi

echo "==> Building tasty ($PROFILE)..."
cargo build $CARGO_FLAGS

echo "==> Assembling archive..."
rm -rf "$DIST_DIR/$PKG_DIR"
mkdir -p "$DIST_DIR/$PKG_DIR"

cp "target/$PROFILE/tasty" "$DIST_DIR/$PKG_DIR/tasty"

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
    mkdir -p "$APPDIR"
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
