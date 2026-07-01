#!/usr/bin/env bash
# ensure-sign-key.sh — release/dist 빌드용 Ed25519 서명 키 경로 결정 (공용 헬퍼).
#
# 키 탐색 규칙 (Justfile + dist 스크립트가 공유):
#   SIGN_KEY_PATH (env)            — 이미 지정돼 있으면 그대로 사용 (CI 등)
#   → ~/.tasty-keys/release.pem    — 있으면 release 키
#   → ~/.tasty-keys/dev.pem        — 없으면 gen-dev-key.sh 로 생성 후 dev 키
#
# 출력:
#   stdout — 결정된 키 경로 **한 줄만**. (caller 가 KEY=$(ensure-sign-key.sh) 로 캡처)
#   stderr — 모든 진단/안내 메시지.
#
# 부작용:
#   dev 키 경로로 떨어질 때 gen-dev-key.sh 를 호출한다. gen-dev-key.sh 는 idempotent
#   하므로 dev.pem 이 이미 있으면 유지하고 추적되지 않는 dev-pubkey.bin 만 (변경 시)
#   재도출한다 → 빌드가 서명 키와 일치하는 trust 키를 임베드한다.
#
# 호출 측에서 profile == debug 면 이 헬퍼를 부르지 않는다 (게이트가 꺼져 서명 불필요).

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

# dev 키 경로: 없으면 생성, 있으면 dev-pubkey.bin 만 재도출 (idempotent).
echo "==> Ensuring dev signing key + embedded pubkey..." >&2
bash "$SCRIPT_DIR/gen-dev-key.sh" >&2
echo "$DEV_KEY"
