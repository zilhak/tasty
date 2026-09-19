#!/usr/bin/env bash
#
# 정적 검사기(shellcheck)를 `~/.local/bin` 에 놓는다 — 멱등. 이미 있으면 아무것도 안 한다.
#
# 첫 낱말이 검사기 이름이면 그 주석 줄 자체가 **지시자**로 읽힌다(SC1072/SC1073). 이
# 파일이 그 형태로 시작했다가 게이트에 잡혔다 — 게이트가 자기 설치 스크립트를 처음으로
# 잡은 것이고, 그래서 그 자리에 이 문장이 있다.
#
# 왜 이 스크립트가 필요한가: `scripts/check-shell-assets.sh` 는 검사기가 없으면
# **판정 불가로 실패한다**(통과가 아니다). 그 규율이 서려면 검사기를 얻는 길이 한 줄로
# 있어야 한다 — 없으면 그 게이트는 "고칠 수 없는 빨강" 이 되고, 그러면 사람이 게이트를
# 끄는 쪽으로 간다.
#
# 배포판 패키지로 받아도 똑같이 동작한다(apt/brew/pacman 의 shellcheck). 이 스크립트는
# 그것이 없거나 sudo 가 없는 환경 — 특히 CI 러너 — 을 위한 길이다.
#
# 버전을 박는 이유: 정적 검사기는 판**이 바뀌면 찾는 것도 바뀐다.** 게이트가 잔여 0 을
# 요구하므로, 판이 조용히 올라가면 어느 날 아무 커밋과도 무관하게 빨개진다. 올릴 때는
# 그 회차가 새로 잡는 것을 보고 올린다.

set -e -o pipefail

VERSION="v0.11.0"
PREFIX="${PREFIX:-$HOME/.local/bin}"

# 아카이브의 sha256. 받은 것이 발행된 것과 같은지 본다 — 검사기 자신을 네트워크에서
# 받으면서 무엇을 받았는지 안 묻는 것은, 판정을 남의 손에 맡기는 것이다.
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
