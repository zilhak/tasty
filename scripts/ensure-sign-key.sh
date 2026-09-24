#!/usr/bin/env bash
# 서명 키 경로만 stdout으로 반환한다. 환경변수, release 키, dev 키 순으로 고른다.
# 환경변수로 지정한 경로는 여기서 존재 여부를 검사하지 않는다.
# dev 키를 고르면 gen-dev-key.sh가 키 생성과 공개키 파일 갱신을 수행한다.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [[ -n "${SIGN_KEY_PATH:-}" ]]; then
    echo "$SIGN_KEY_PATH"
    exit 0
fi

RELEASE_KEY="$HOME/.tasty-keys/release.pem"
DEV_KEY="$HOME/.tasty-keys/dev.pem"

if [[ -f "$RELEASE_KEY" ]]; then
    echo "$RELEASE_KEY"
    exit 0
fi

echo "==> Ensuring dev signing key + embedded pubkey..." >&2
bash "$SCRIPT_DIR/gen-dev-key.sh" >&2
echo "$DEV_KEY"
