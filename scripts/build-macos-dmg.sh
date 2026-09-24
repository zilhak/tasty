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
#   dist/Tasty-{version}-macos-arm64.dmg — the disk image (Apple Silicon only)

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "Error: This script must be run on macOS." >&2
    exit 1
fi

cd "$(dirname "$0")/.."

# 이 배포 스크립트는 Apple Silicon 타깃만 만든다.
TARGET=aarch64-apple-darwin
installed_targets=$(rustup target list --installed 2>/dev/null || true)
if ! grep -qx "$TARGET" <<<"$installed_targets"; then
    echo "Error: rust target '$TARGET' not installed. Run: rustup target add $TARGET" >&2
    exit 1
fi
command -v codesign &>/dev/null || {
    echo "Error: 'codesign' not found (part of Xcode Command Line Tools)." >&2
    exit 1
}

PROFILE="dist"
CARGO_FLAGS="--profile dist"
if [[ "${1:-}" == "--debug" ]]; then
    PROFILE="debug"
    CARGO_FLAGS=""
elif [[ "${1:-}" == "--release" ]]; then
    PROFILE="release"
    CARGO_FLAGS="--release"
fi

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
APP_NAME="Tasty"
BUNDLE_ID="com.zilhak.tasty"
DIST_DIR="dist"
APP_DIR="$DIST_DIR/$APP_NAME.app"
DMG_NAME="$APP_NAME-$VERSION-macos-arm64.dmg"

# 빌드 전에 서명 키를 준비해 대응 공개키가 바이너리에 포함되게 한다.
if [[ "$PROFILE" != "debug" ]]; then
    SIGN_KEY_PATH="$(./scripts/ensure-sign-key.sh)"
    export SIGN_KEY_PATH
fi

echo "==> Building tasty ($PROFILE) for $TARGET..."
cargo build $CARGO_FLAGS --target "$TARGET"

# 매니페스트가 있고 bundle=false가 아닌 plugin을 배포에 포함한다.
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

echo "==> Building ${#PLUGIN_CRATES[@]} plugins ($PROFILE) for $TARGET..."
PLUGIN_CARGO_ARGS=()
for c in "${PLUGIN_CRATES[@]}"; do
    PLUGIN_CARGO_ARGS+=("-p" "$c")
done
cargo build $CARGO_FLAGS --target "$TARGET" "${PLUGIN_CARGO_ARGS[@]}"

# $1 = 바이너리 이름 (target/$TARGET/$PROFILE/<name> 아래에서 찾음), $2 = 출력 경로.
stage_binary() {
    local name="$1" out="$2"
    local src="target/$TARGET/$PROFILE/$name"
    if [[ ! -f "$src" ]]; then
        echo "Error: binary missing: $src" >&2
        exit 1
    fi
    cp "$src" "$out"
}

if [[ "$PROFILE" != "debug" ]]; then
    echo "==> Signing plugin manifests with $SIGN_KEY_PATH..."
    ./scripts/sign-bundle.sh --key "$SIGN_KEY_PATH" --all-builtins
fi

echo "==> Assembling $APP_NAME.app..."
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

stage_binary tasty "$APP_DIR/Contents/MacOS/tasty"

# plugin 디렉터리를 별도 코드 번들로 해석하지 않도록 Contents/MacOS 대신 Resources에 둔다.
PLUGINS_DIR="$APP_DIR/Contents/Resources/plugins"
mkdir -p "$PLUGINS_DIR"
for c in "${PLUGIN_CRATES[@]}"; do
    manifest="crates/$c/tasty-plugin.toml"
    id=$(grep -m1 -E '^id[[:space:]]*=' "$manifest" | sed 's/.*"\([^"]*\)".*/\1/')
    if [[ -z "$id" ]]; then
        echo "Error: cannot parse id from $manifest" >&2
        exit 1
    fi
    dest="$PLUGINS_DIR/$id"
    mkdir -p "$dest"
    stage_binary "$c" "$dest/$c"
    cp "$manifest" "$dest/tasty-plugin.toml"
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

cp "assets/icons/icon.icns" "$APP_DIR/Contents/Resources/icon.icns"

# 서명 후 내용을 바꾸면 서명이 깨지므로 notice 파일도 먼저 배치한다.
# shellcheck source=scripts/lib/notice-set.sh
. "scripts/lib/notice-set.sh"
stage_notice "$APP_DIR/Contents/Resources"

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
    <!-- TCC usage descriptions. macOS 는 보호 리소스 접근 프롬프트 본문에 이 문자열을
         그대로 띄운다. 키가 없으면 이유 없는 프롬프트가 뜨고, 일부 서비스는 접근
         시도 자체가 즉시 실패한다. 터미널은 사용자가 친 명령을 대신 실행하는 것이
         본업이라, 어느 폴더에 닿을지는 명령에 달려 있다 — 그 사실을 문구에 담는다.
         (다국어화는 Contents/Resources 의 lang.lproj/InfoPlist.strings 가 필요한 별개
         메커니즘이라 여기서는 개발 지역 기본값인 영어로 둔다.) -->
    <key>NSDownloadsFolderUsageDescription</key>
    <string>Tasty runs the commands you type in its terminal. Grant access so those commands can read and write files in your Downloads folder.</string>
    <key>NSDocumentsFolderUsageDescription</key>
    <string>Tasty runs the commands you type in its terminal. Grant access so those commands can read and write files in your Documents folder.</string>
    <key>NSDesktopFolderUsageDescription</key>
    <string>Tasty runs the commands you type in its terminal. Grant access so those commands can read and write files on your Desktop.</string>
    <key>NSRemovableVolumesUsageDescription</key>
    <string>Tasty runs the commands you type in its terminal. Grant access so those commands can read and write files on removable volumes such as USB drives and SD cards.</string>
    <key>NSNetworkVolumesUsageDescription</key>
    <string>Tasty runs the commands you type in its terminal. Grant access so those commands can read and write files on mounted network volumes.</string>
</dict>
</plist>
PLIST

# 번들을 완성한 뒤 서명한다. 이 작업은 공증을 포함하지 않는다.
# 명시한 TASTY_CODESIGN_IDENTITY가 없으면 아래 조건에 따라 개발 인증서 또는 ad-hoc을 선택한다.
DEV_IDENTITY_NAME="Tasty Dev"
SIGN_IDENTITY="${TASTY_CODESIGN_IDENTITY:-}"
if [[ -z "$SIGN_IDENTITY" ]]; then
    # 로컬 설치(NO_DMG=1)에만 개발 인증서를 자동 선택한다. 배포 DMG에 로컬 인증서를 넣지 않기 위해서다.
    codesign_identities=$(security find-identity -v -p codesigning 2>/dev/null || true)
    if [[ "${NO_DMG:-}" == "1" ]] &&
        grep -q "\"$DEV_IDENTITY_NAME\"" <<<"$codesign_identities"; then
        SIGN_IDENTITY="$DEV_IDENTITY_NAME"
    else
        SIGN_IDENTITY="-"
    fi
fi
if [[ "$SIGN_IDENTITY" == "-" ]]; then
    echo "==> Ad-hoc signing $APP_NAME.app..."
    if [[ "${NO_DMG:-}" == "1" ]]; then
        echo "    (TCC 승인을 재빌드 후에도 유지하려면 ./scripts/macos-codesign-identity.sh 참고)"
    fi
else
    echo "==> Signing $APP_NAME.app with identity '$SIGN_IDENTITY'..."
fi
codesign --force --sign "$SIGN_IDENTITY" "$APP_DIR"

echo "==> Verifying $APP_NAME.app..."
"$APP_DIR/Contents/MacOS/tasty" --version >/dev/null || {
    echo "Error: binary failed to invoke --version" >&2
    exit 1
}
ARCH_LINE=$(file "$APP_DIR/Contents/MacOS/tasty")
[[ "$ARCH_LINE" == *"Mach-O"*"arm64"* ]] || {
    echo "Error: not an arm64 Mach-O binary: $ARCH_LINE" >&2
    exit 1
}
# linker의 자동 서명과 구별하려고 bundle 봉인·Identifier·linker-signed 여부를 함께 확인한다.
if [[ ! -d "$APP_DIR/Contents/_CodeSignature" ]]; then
    echo "Error: $APP_NAME.app has no _CodeSignature (codesign did not run)" >&2
    exit 1
fi
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
verify_notice_tree "$APP_DIR/Contents/Resources" "$APP_NAME.app" || exit 1
PLIST_VER=$(plutil -extract CFBundleVersion raw "$APP_DIR/Contents/Info.plist")
[[ "$PLIST_VER" == "$VERSION" ]] || {
    echo "Error: Info.plist version $PLIST_VER != Cargo.toml $VERSION" >&2
    exit 1
}

# NO_DMG=1이면 완성된 app까지만 만들고 복사·설치는 호출자에게 맡긴다.
if [[ "${NO_DMG:-}" == "1" ]]; then
    echo "==> NO_DMG set — skipping DMG packaging."
    echo "  App:  $APP_DIR"
    exit 0
fi

echo "==> Creating $DMG_NAME..."
rm -f "$DIST_DIR/$DMG_NAME"

DMG_STAGE="$DIST_DIR/dmg-stage"
rm -rf "$DMG_STAGE"
mkdir -p "$DMG_STAGE"
cp -R "$APP_DIR" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"
verify_notice_tree "$DMG_STAGE/$APP_NAME.app/Contents/Resources" "$DMG_STAGE" || exit 1

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
# 입력 디렉터리뿐 아니라 완성 DMG를 읽기 전용으로 열어 notice 파일을 대조한다.
DMG_MOUNT=$(mktemp -d)
hdiutil attach -nobrowse -readonly -noautoopen -mountpoint "$DMG_MOUNT" "$DIST_DIR/$DMG_NAME" >/dev/null
dmg_notice_rc=0
verify_notice_tree "$DMG_MOUNT/$APP_NAME.app/Contents/Resources" "$DMG_NAME" || dmg_notice_rc=$?
hdiutil detach "$DMG_MOUNT" >/dev/null || echo "Warning: could not detach $DMG_MOUNT" >&2
rmdir "$DMG_MOUNT" 2>/dev/null || true
[[ "$dmg_notice_rc" -eq 0 ]] || exit 1

SHASUMS_FILE="SHA256SUMS-macos.txt"
(cd "$DIST_DIR" && shasum -a 256 "$DMG_NAME" > "$SHASUMS_FILE")

echo ""
echo "Done!"
echo "  App:  $APP_DIR"
echo "  DMG:  $DIST_DIR/$DMG_NAME"
echo "  SHA:  $DIST_DIR/$SHASUMS_FILE"
