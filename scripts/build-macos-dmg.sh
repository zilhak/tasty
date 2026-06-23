#!/bin/bash
# Build a macOS .app bundle and .dmg disk image for Tasty.
#
# Usage:
#   ./scripts/build-macos-dmg.sh           # dist build (full LTO, 배포용)
#   ./scripts/build-macos-dmg.sh --release # release build (thin LTO, 빠른 빌드)
#   ./scripts/build-macos-dmg.sh --debug   # debug build
#
# Output:
#   dist/Tasty.app    — the application bundle
#   dist/Tasty.dmg    — the disk image (ready to distribute)

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must be run on macOS." >&2
    exit 1
fi

cd "$(dirname "$0")/.."

# Parse arguments
# 기본은 dist 프로필 (full LTO)을 써서 가장 빠른 바이너리를 배포한다.
# 개발 중 빠르게 .app만 만들고 싶을 땐 --release(thin LTO) 또는 --debug 사용.
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
APP_NAME="Tasty"
BUNDLE_ID="com.zilhak.tasty"
DIST_DIR="dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"
DMG_NAME="$APP_NAME-$VERSION-macos.dmg"

# Discover signing key BEFORE cargo build — so that the matching dev-pubkey.bin
# is (re)derived and embedded into the host-plugin binary at compile time.
# release/dist builds require a signing key; debug skips entirely.
if [[ "$PROFILE" != "debug" ]]; then
    if [[ -z "${SIGN_KEY_PATH:-}" ]]; then
        if [[ -f "$HOME/.tasty-keys/release.pem" ]]; then
            SIGN_KEY_PATH="$HOME/.tasty-keys/release.pem"
        else
            # dev 키 경로: 없으면 생성, 있으면 추적되지 않는 dev-pubkey.bin 을
            # dev.pem 에서 재도출한다. gen-dev-key.sh 가 두 경우 모두 처리
            # (idempotent) → 빌드가 all-zero placeholder 대신 서명 키와 일치하는
            # trust 키를 임베드한다.
            echo "==> Ensuring dev signing key + embedded pubkey..."
            ./scripts/gen-dev-key.sh
            SIGN_KEY_PATH="$HOME/.tasty-keys/dev.pem"
        fi
    fi
    export SIGN_KEY_PATH
fi

echo "==> Building tasty ($PROFILE)..."
cargo build $CARGO_FLAGS

# Discover bundled plugin crates (any `crates/tasty-plugin-*` with a manifest).
# Matches justfile `build-plugins` recipe — keep them in sync.
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

echo "==> Assembling $APP_NAME.app..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy executable
cp "target/$PROFILE/tasty" "$APP_DIR/Contents/MacOS/tasty"

# Stage plugins next to the executable. `bundle_root()` (crates/tasty-host-plugin/
# src/builtin.rs) discovers `<exe_dir>/plugins/` for packaged builds and syncs each
# `<plugin-id>/` into `~/.tasty/plugins/<id>/` on first launch.
PLUGINS_DIR="$APP_DIR/Contents/MacOS/plugins"
mkdir -p "$PLUGINS_DIR"
for c in "${PLUGIN_CRATES[@]}"; do
    manifest="crates/$c/tasty-plugin.toml"
    id=$(grep -E '^id[[:space:]]*=' "$manifest" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
    if [[ -z "$id" ]]; then
        echo "Error: cannot parse id from $manifest" >&2
        exit 1
    fi
    src_bin="target/$PROFILE/$c"
    if [[ ! -f "$src_bin" ]]; then
        echo "Error: plugin binary missing: $src_bin" >&2
        exit 1
    fi
    dest="$PLUGINS_DIR/$id"
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

# Copy icon
cp "assets/icons/icon.icns" "$APP_DIR/Contents/Resources/icon.icns"

# Generate Info.plist
cat > "$APP_DIR/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>$APP_NAME</string>
    <key>CFBundleDisplayName</key>
    <string>$APP_NAME</string>
    <key>CFBundleIdentifier</key>
    <string>$BUNDLE_ID</string>
    <key>CFBundleVersion</key>
    <string>$VERSION</string>
    <key>CFBundleShortVersionString</key>
    <string>$VERSION</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleExecutable</key>
    <string>tasty</string>
    <key>CFBundleIconFile</key>
    <string>icon</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

echo "==> Creating $DMG_NAME..."
rm -f "$DIST_DIR/$DMG_NAME"

# Stage DMG contents: app + Applications symlink for drag-install
DMG_STAGE="$DIST_DIR/dmg-stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"

hdiutil create -volname "$APP_NAME" \
    -srcfolder "$DMG_STAGE" \
    -ov -format UDZO \
    "$DIST_DIR/$DMG_NAME"

rm -rf "$DMG_STAGE"

echo "==> Verifying artifacts..."
"$APP_DIR/Contents/MacOS/tasty" --version >/dev/null || {
    echo "Error: binary failed to invoke --version" >&2
    exit 1
}
ARCH_LINE=$(file "$APP_DIR/Contents/MacOS/tasty")
[[ "$ARCH_LINE" == *"Mach-O"* ]] || {
    echo "Error: not a Mach-O binary: $ARCH_LINE" >&2
    exit 1
}
PLIST_VER=$(plutil -extract CFBundleVersion raw "$APP_DIR/Contents/Info.plist")
[[ "$PLIST_VER" == "$VERSION" ]] || {
    echo "Error: Info.plist version $PLIST_VER != Cargo.toml $VERSION" >&2
    exit 1
}
[[ -f "$DIST_DIR/$DMG_NAME" ]] || {
    echo "Error: DMG missing: $DIST_DIR/$DMG_NAME" >&2
    exit 1
}

SHASUMS_FILE="SHA256SUMS-macos.txt"
(cd "$DIST_DIR" && shasum -a 256 "$DMG_NAME" > "$SHASUMS_FILE")

echo ""
echo "Done!"
echo "  App:  $APP_DIR"
echo "  DMG:  $DIST_DIR/$DMG_NAME"
echo "  SHA:  $DIST_DIR/$SHASUMS_FILE"
