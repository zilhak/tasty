#!/bin/bash
# Build Tasty (host + plugins, dist profile) and install it to /Applications.
#
# Usage:
#   ./scripts/install-macos.sh            # dist build (full LTO, 배포용)
#   ./scripts/install-macos.sh --release  # release build (thin LTO, 빠른 빌드)
#   ./scripts/install-macos.sh --debug    # debug build
#
# Reuses build-macos-dmg.sh to assemble dist/Tasty.app (NO_DMG skips packaging),
# then overwrites /Applications/Tasty.app with the freshly built bundle. The
# bundled plugins ride inside the .app (Contents/MacOS/plugins/) and the host
# force-overwrites ~/.tasty/plugins/<id>/ on first launch — so after install the
# app body AND every plugin are at the latest built version.

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must be run on macOS." >&2
    exit 1
fi

cd "$(dirname "$0")/.."

APP_NAME="Tasty"
SRC_APP="dist/$APP_NAME.app"
DEST_APP="/Applications/$APP_NAME.app"

# Assemble the .app bundle without packaging a DMG (NO_DMG=1).
NO_DMG=1 ./scripts/build-macos-dmg.sh "$@"

if [[ ! -d "$SRC_APP" ]]; then
    echo "Error: expected bundle missing: $SRC_APP" >&2
    exit 1
fi

echo "==> Installing to $DEST_APP (overwrite)..."
rm -rf "$DEST_APP"
cp -R "$SRC_APP" "$DEST_APP"

echo ""
echo "Installed!"
echo "  App: $DEST_APP"
echo "  플러그인은 앱 첫 실행 시 ~/.tasty/plugins 로 강제 덮어쓰기 동기화됩니다."
