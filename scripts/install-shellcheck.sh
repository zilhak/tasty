#!/usr/bin/env bash
# ShellCheck가 PATH에 없으면 고정 버전·SHA256의 Linux 바이너리를 설치한다.
# 기존 명령이 있으면 버전을 바꾸지 않는다. PREFIX로 설치 디렉터리를 지정할 수 있다.

set -e -o pipefail

VERSION="v0.11.0"
PREFIX="${PREFIX:-$HOME/.local/bin}"

# 다운로드한 아카이브를 아래 고정 SHA256과 대조한다.
SHA_x86_64="8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198"
SHA_aarch64="12b331c1d2db6b9eb13cfca64306b1b157a86eb69db83023e261eaa7e7c14588"

if command -v shellcheck >/dev/null 2>&1; then
    echo "[install-shellcheck] 이미 있다: $(command -v shellcheck) ($(shellcheck --version | sed -n 's/^version: //p'))"
    exit 0
fi

OS="$(uname -s)"
[ "$OS" = Linux ] || {
    echo "[install-shellcheck] 이 스크립트는 Linux 용이다 (지금 $OS)." >&2
    echo "  macOS: brew install shellcheck" >&2
    exit 2
}

ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  WANT="$SHA_x86_64" ;;
    aarch64) WANT="$SHA_aarch64" ;;
    *) echo "[install-shellcheck] 발행 아카이브가 없는 아키텍처다: $ARCH — 배포판 패키지를 써라." >&2; exit 2 ;;
esac

TAR="shellcheck-$VERSION.linux.$ARCH.tar.xz"
URL="https://github.com/koalaman/shellcheck/releases/download/$VERSION/$TAR"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP:?}"' EXIT

echo "[install-shellcheck] 받는다: $URL"
curl -fsSL -o "$TMP/$TAR" "$URL" || { echo "[install-shellcheck] 내려받기 실패." >&2; exit 2; }

GOT="$(sha256sum "$TMP/$TAR" | cut -d' ' -f1)"
[ "$GOT" = "$WANT" ] || {
    echo "[install-shellcheck] sha256 이 다르다 — 설치하지 않는다." >&2
    echo "  기대 $WANT" >&2
    echo "  실제 $GOT" >&2
    exit 2
}

tar -xf "$TMP/$TAR" -C "$TMP"
mkdir -p "$PREFIX"
install -m 0755 "$TMP/shellcheck-$VERSION/shellcheck" "$PREFIX/shellcheck"

echo "[install-shellcheck] 놓았다: $PREFIX/shellcheck ($("$PREFIX/shellcheck" --version | sed -n 's/^version: //p'))"
command -v shellcheck >/dev/null 2>&1 || {
    echo "[install-shellcheck] ▸ $PREFIX 가 PATH 에 없다. 셸 설정에 더해라:" >&2
    echo "    export PATH=\"$PREFIX:\$PATH\"" >&2
}
