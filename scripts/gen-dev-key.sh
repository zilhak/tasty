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
#   3. dev-pubkey.bin 은 추적하지 않는 로컬 전용 파일 (keys/.gitignore).
#      build.rs 가 OUT_DIR 로 staging 하므로 커밋하지 않는다.

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

# raw 32 byte Ed25519 public key 를 private key 에서 추출해 $2 에 기록.
# openssl 의 DER 출력 마지막 32 byte 가 raw key
# (44 byte DER = 12 byte SubjectPublicKeyInfo prefix + 32 byte key).
#
# dev-pubkey.bin 은 추적되지 않는 로컬 전용 파일이다 (build.rs 가 OUT_DIR 로
# staging). private key 가 이미 있어도 빌드 전 이 함수로 매번 재도출해야
# 임베드되는 trust 키가 서명 키와 일치한다. 누락 시 build.rs 가 all-zero
# placeholder 를 임베드해 dev 서명 plugin 이 전부 거부된다.
derive_pubkey() {
    local priv="$1" out="$2"
    mkdir -p "$(dirname "$out")"
    local tmp
    tmp="$(mktemp)"
    openssl pkey -in "$priv" -pubout -outform DER | tail -c 32 > "$tmp"
    local len
    len=$(wc -c < "$tmp" | tr -d ' ')
    if [[ "$len" != "32" ]]; then
        echo "Error: extracted pubkey is $len bytes, expected 32" >&2
        echo "  (openssl 버전 또는 OS 차이로 인한 DER 헤더 변경 가능성)" >&2
        rm -f "$tmp"
        exit 1
    fi
    # 내용이 동일하면 mtime 을 건드리지 않는다. dev-pubkey.bin 은 build.rs 의
    # rerun-if-changed 대상이라, 매번 재기록하면 내용이 같아도 tasty-host-plugin +
    # tasty 바이너리가 불필요하게 재컴파일된다. 부재/내용 불일치일 때만 갱신.
    if [[ -f "$out" ]] && cmp -s "$tmp" "$out"; then
        rm -f "$tmp"
    else
        mv "$tmp" "$out"
    fi
}

if [[ -f "$PRIV_PATH" && "$FORCE" -ne 1 ]]; then
    # private key 는 유지하되, 추적되지 않는 공개키는 항상 (재)도출한다.
    derive_pubkey "$PRIV_PATH" "$PUB_PATH"
    echo "Private key exists: $PRIV_PATH (kept)"
    echo "Re-derived public key: $PUB_PATH"
    echo "  Use --force to regenerate the private key (NOTE: 기존 .sig 는 모두 무효화됨)."
    exit 0
fi

# 1) Ed25519 private key 생성.
openssl genpkey -algorithm Ed25519 -out "$PRIV_PATH"
chmod 600 "$PRIV_PATH"

# 2) raw 32 byte public key 추출.
derive_pubkey "$PRIV_PATH" "$PUB_PATH"

cat <<EOF
==> Generated Ed25519 dev keypair.

  Private (gitignored, chmod 600): $PRIV_PATH
  Public  (local, untracked):      $PUB_PATH

다음 단계:
  ./scripts/sign-bundle.sh --key "$PRIV_PATH" --all-builtins
  cargo build -p tasty-host-plugin    # 새 dev-pubkey.bin 이 컴파일타임 임베드됨
EOF
