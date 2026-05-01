#!/usr/bin/env bash
# Build a Linux tar.gz archive for Tasty.
#
# Usage:
#   ./scripts/build-linux.sh           # dist build (full LTO, 배포용)
#   ./scripts/build-linux.sh --release # release build (thin LTO, 빠른 빌드)
#   ./scripts/build-linux.sh --debug   # debug build
#
# Output:
#   dist/tasty-{version}-linux-x64.tar.gz

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

VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
DIST_DIR="dist"
ARCHIVE_NAME="tasty-${VERSION}-linux-x64.tar.gz"

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
rm -rf "$DIST_DIR/tasty-linux"
mkdir -p "$DIST_DIR/tasty-linux"

cp "target/$PROFILE/tasty" "$DIST_DIR/tasty-linux/tasty"

echo "==> Creating $ARCHIVE_NAME..."
rm -f "$DIST_DIR/$ARCHIVE_NAME"
tar -czf "$DIST_DIR/$ARCHIVE_NAME" -C "$DIST_DIR" "tasty-linux"

rm -rf "$DIST_DIR/tasty-linux"

echo ""
echo "Done!"
echo "  Archive: $DIST_DIR/$ARCHIVE_NAME"
