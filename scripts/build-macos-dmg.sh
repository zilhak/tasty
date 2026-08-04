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

# Universal binary(Apple Silicon + Intel 모두 배포) 빌드에 필요한 두 타깃 + 도구.
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
for t in "${TARGETS[@]}"; do
    if ! rustup target list --installed 2>/dev/null | grep -qx "$t"; then
        echo "Error: rust target '$t' not installed. Run: rustup target add $t" >&2
        exit 1
    fi
done
for tool in lipo codesign; do
    command -v "$tool" &>/dev/null || {
        echo "Error: '$tool' not found (part of Xcode Command Line Tools)." >&2
        exit 1
    }
done

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
# Key-discovery rule is shared with the Justfile via scripts/ensure-sign-key.sh.
if [[ "$PROFILE" != "debug" ]]; then
    SIGN_KEY_PATH="$(./scripts/ensure-sign-key.sh)"
    export SIGN_KEY_PATH
fi

echo "==> Building tasty ($PROFILE) for ${TARGETS[*]}..."
for t in "${TARGETS[@]}"; do
    cargo build $CARGO_FLAGS --target "$t"
done

# Discover bundled plugin crates (any `crates/tasty-plugin-*` with a manifest).
# Matches build-linux.sh / build-windows.ps1 — keep in sync. A manifest with
# `bundle = false` (demo/PoC plugins) is skipped from distribution; dev staging
# (`just build-plugins`/`link-plugins`) still includes it.
PLUGIN_CRATES=()
for d in crates/tasty-plugin-*; do
    [ -f "$d/tasty-plugin.toml" ] || continue
    if grep -Eq '^[[:space:]]*bundle[[:space:]]*=[[:space:]]*false' "$d/tasty-plugin.toml"; then
        echo "==> Skipping $(basename "$d") (bundle = false)"
        continue
    fi
    PLUGIN_CRATES+=("$(basename "$d")")
done

if [[ ${#PLUGIN_CRATES[@]} -eq 0 ]]; then
    echo "Error: no plugin crates with tasty-plugin.toml found under crates/" >&2
    exit 1
fi

echo "==> Building ${#PLUGIN_CRATES[@]} plugins ($PROFILE) for ${TARGETS[*]}..."
PLUGIN_CARGO_ARGS=()
for c in "${PLUGIN_CRATES[@]}"; do
    PLUGIN_CARGO_ARGS+=("-p" "$c")
done
for t in "${TARGETS[@]}"; do
    cargo build $CARGO_FLAGS --target "$t" "${PLUGIN_CARGO_ARGS[@]}"
done

# Plugin 은 별도 프로세스로 spawn 되므로 host 와 마찬가지로 universal 이어야 한다
# (item 1 열린 질문: 기본은 host 와 동일하게 둘 다 lipo — 빌드 시간이 실제 문제가
# 되면 그때 가서 판단, 여기서 임의로 스킵하지 않는다).
# $1 = 바이너리 이름 (target/<triple>/$PROFILE/<name> 아래에서 찾음), $2 = 출력 경로.
lipo_universal() {
    local name="$1" out="$2"
    lipo -create \
        "target/${TARGETS[0]}/$PROFILE/$name" \
        "target/${TARGETS[1]}/$PROFILE/$name" \
        -output "$out"
}

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

# lipo 로 두 arch 를 하나의 universal Mach-O 로 합쳐 배치.
lipo_universal tasty "$APP_DIR/Contents/MacOS/tasty"

# Stage plugins under Contents/Resources/. `bundle_root()` (crates/tasty-host-plugin/
# src/builtin.rs) discovers `Contents/Resources/plugins/` for .app bundles and syncs
# each `<plugin-id>/` into `~/.tasty/plugins/<id>/` on first launch.
#
# NOT Contents/MacOS/ — codesign treats any directory holding an executable under
# Contents/MacOS/ as nested code and tries to parse it as a bundle. These plugin
# directories have no Contents/Info.plist, so signing fails outright with
# "bundle format unrecognized, invalid, or unsuitable". Under Contents/Resources/
# they are sealed as ordinary resources and the bundle signs cleanly.
PLUGINS_DIR="$APP_DIR/Contents/Resources/plugins"
mkdir -p "$PLUGINS_DIR"
for c in "${PLUGIN_CRATES[@]}"; do
    manifest="crates/$c/tasty-plugin.toml"
    id=$(grep -E '^id[[:space:]]*=' "$manifest" | head -1 | sed 's/.*"\([^"]*\)".*/\1/')
    if [[ -z "$id" ]]; then
        echo "Error: cannot parse id from $manifest" >&2
        exit 1
    fi
    for t in "${TARGETS[@]}"; do
        src_bin="target/$t/$PROFILE/$c"
        if [[ ! -f "$src_bin" ]]; then
            echo "Error: plugin binary missing: $src_bin" >&2
            exit 1
        fi
    done
    dest="$PLUGINS_DIR/$id"
    mkdir -p "$dest"
    lipo_universal "$c" "$dest/$c"
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

# 코드 서명 (ad-hoc — 무료, Apple 계정 불요). Apple Silicon 의 "damaged/can't open"
# 하드 블록을 완화한다. lipo·plugin staging·Info.plist 생성 *이후*, DMG 생성 이전 —
# 서명 뒤에 번들 내용을 바꾸면 서명이 깨진다. Gatekeeper "미확인 개발자" 경고는
# 이것으로 없어지지 않는다(공증 기각은 정책 결정 — docs 에서 우회 안내).
#
# --deep 은 쓰지 않는다 (Apple 이 deprecate 했고, 여기선 불필요) — plugin 은
# Contents/Resources/ 아래의 일반 리소스로 봉인된다.
echo "==> Ad-hoc signing $APP_NAME.app..."
codesign --force --sign - "$APP_DIR"

# Verify the assembled .app (both install and DMG paths share this).
echo "==> Verifying $APP_NAME.app..."
"$APP_DIR/Contents/MacOS/tasty" --version >/dev/null || {
    echo "Error: binary failed to invoke --version" >&2
    exit 1
}
ARCH_LINE=$(file "$APP_DIR/Contents/MacOS/tasty")
[[ "$ARCH_LINE" == *"Mach-O"* ]] || {
    echo "Error: not a Mach-O binary: $ARCH_LINE" >&2
    exit 1
}
LIPO_ARCHS=$(lipo -archs "$APP_DIR/Contents/MacOS/tasty")
[[ "$LIPO_ARCHS" == *"x86_64"* && "$LIPO_ARCHS" == *"arm64"* ]] || {
    echo "Error: tasty is not a universal (x86_64+arm64) binary: $LIPO_ARCHS" >&2
    exit 1
}
# 서명 검증. `codesign -dv | grep Signature=adhoc` 만으로는 부족하다 — 링커가
# 자동으로 붙이는 서명(linker-signed)도 "Signature=adhoc" 를 출력하므로, codesign
# 이 아예 실행되지 않은 번들도 통과한다. 실제로 서명됐는지는 _CodeSignature/ 봉인,
# Identifier 가 번들 ID 로 잡혔는지, linker-signed 플래그가 사라졌는지로 판별한다.
if [[ ! -d "$APP_DIR/Contents/_CodeSignature" ]]; then
    echo "Error: $APP_NAME.app has no _CodeSignature (codesign did not run)" >&2
    exit 1
fi
# `|| true` — codesign -dv 자체가 실패해도 아래 검사에서 진단 메시지를 내고 죽도록.
CODESIGN_INFO=$(codesign -dv "$APP_DIR" 2>&1 || true)
if ! grep -Fxq "Identifier=$BUNDLE_ID" <<<"$CODESIGN_INFO"; then
    echo "Error: signing identifier is not $BUNDLE_ID:" >&2
    grep "^Identifier=" <<<"$CODESIGN_INFO" >&2 || true
    exit 1
fi
if grep -q "linker-signed" <<<"$CODESIGN_INFO"; then
    echo "Error: $APP_NAME.app carries only the linker's automatic signature" >&2
    exit 1
fi
if ! codesign --verify --deep --strict "$APP_DIR"; then
    echo "Error: $APP_NAME.app failed codesign --verify" >&2
    exit 1
fi
PLIST_VER=$(plutil -extract CFBundleVersion raw "$APP_DIR/Contents/Info.plist")
[[ "$PLIST_VER" == "$VERSION" ]] || {
    echo "Error: Info.plist version $PLIST_VER != Cargo.toml $VERSION" >&2
    exit 1
}

# Install path (NO_DMG=1, used by install-macos.sh): stop after assembling the
# .app; skip DMG packaging. The caller copies dist/Tasty.app to /Applications.
if [[ "${NO_DMG:-}" == "1" ]]; then
    echo "==> NO_DMG set — skipping DMG packaging."
    echo "  App:  $APP_DIR"
    exit 0
fi

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

echo "==> Verifying DMG..."
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
