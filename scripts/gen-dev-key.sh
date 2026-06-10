#!/usr/bin/env bash
# gen-dev-key.sh — 개발자 로컬 Ed25519 dev keypair 생성 + 공개키 임베드 갱신.
#
# 결과:
#   ~/.tasty-keys/dev.pem                        — Ed25519 private key (chmod 600)
#   crates/tasty-host-plugin/keys/dev-pubkey.bin — raw 32 byte public key
#
# 본 스크립트는 한 번만 실행. private key 가 이미 있으면 덮어쓰지 않고 종료
# (실수로 키 분실 방지). 강제 재생성은 `--force` 플래그.
#
# 생성 후 흐름:
#   1. sign-bundle.sh 로 모든 builtin plugin manifest 서명
#   2. dev 빌드 / cargo run 시 tasty 가 dev-pubkey.bin (임베드) 으로 자동 trust
#   3. dev-pubkey.bin 은 git commit 안전 (공개키 = 비밀 아님)

set -euo pipefail

FORCE=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --force)
            FORCE=1
            shift
            ;;
        -h|--help)
            cat <<'USAGE'
Usage: gen-dev-key.sh [--force]

  --force   기존 dev key 가 있어도 덮어쓰기. 평소엔 사용 금지.
USAGE
            exit 0
            ;;
        *)
            echo "Error: unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

if ! command -v openssl >/dev/null 2>&1; then
    echo "Error: openssl not found in PATH" >&2
    exit 1
fi

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
KEY_DIR="${HOME}/.tasty-keys"
PRIV_PATH="${KEY_DIR}/dev.pem"
PUB_PATH="${REPO_ROOT}/crates/tasty-host-plugin/keys/dev-pubkey.bin"

mkdir -p "$KEY_DIR"
chmod 700 "$KEY_DIR"

if [[ -f "$PRIV_PATH" && "$FORCE" -ne 1 ]]; then
    echo "Already exists: $PRIV_PATH"
    echo "  Use --force to regenerate (NOTE: 기존 키로 서명된 .sig 는 모두 무효화됨)"
    exit 0
fi

# 1) Ed25519 private key 생성.
openssl genpkey -algorithm Ed25519 -out "$PRIV_PATH"
chmod 600 "$PRIV_PATH"

# 2) raw 32 byte public key 추출. openssl 의 DER 출력 마지막 32 byte 가
#    raw key (44 byte DER = 12 byte SubjectPublicKeyInfo prefix + 32 byte key).
mkdir -p "$(dirname "$PUB_PATH")"
openssl pkey -in "$PRIV_PATH" -pubout -outform DER | tail -c 32 > "$PUB_PATH"

# 3) 길이 검증.
pub_len=$(wc -c < "$PUB_PATH" | tr -d ' ')
if [[ "$pub_len" != "32" ]]; then
    echo "Error: extracted pubkey is $pub_len bytes, expected 32" >&2
    echo "  (openssl 버전 또는 OS 차이로 인한 DER 헤더 변경 가능성)" >&2
    rm -f "$PUB_PATH"
    exit 1
fi

cat <<EOF
==> Generated Ed25519 dev keypair.

  Private (gitignored, chmod 600): $PRIV_PATH
  Public  (commit safe):           $PUB_PATH

다음 단계:
  ./scripts/sign-bundle.sh --key "$PRIV_PATH" --all-builtins
  cargo build -p tasty-host-plugin    # 새 dev-pubkey.bin 이 컴파일타임 임베드됨
EOF
