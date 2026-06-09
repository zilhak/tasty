#!/usr/bin/env bash
# sign-bundle.sh — plugin 매니페스트 Ed25519 서명 생성기.
#
# 입력:
#   --key <path>       Ed25519 private key (PEM 형식). 필수.
#   --manifest <path>  단일 매니페스트 (tasty-plugin.toml) 지정.
#   --all-builtins     crates/tasty-plugin-*/tasty-plugin.toml 전부 자동 검색.
#
# 출력:
#   <manifest>.sig — raw 64 byte Ed25519 detached signature.
#
# 알고리즘:
#   1. SHA-256(manifest) → 32 byte digest
#   2. Ed25519 sign(digest) → 64 byte signature
#
# 본 두 단계 hash 는 bundle_sig.rs 의 검증 흐름 (`vk.verify(digest, sig)`)
# 과 정확히 동기화. raw manifest 직접 서명 또는 Ed25519ph (pre-hashed) 로
# 바꾸려면 검증기와 함께 변경해야 함.
#
# 사용 예:
#   ./scripts/sign-bundle.sh --key ~/.tasty-keys/dev.pem \
#       --manifest crates/tasty-plugin-claude/tasty-plugin.toml
#   ./scripts/sign-bundle.sh --key ~/.tasty-keys/dev.pem --all-builtins
#
# 의존: openssl (Ed25519 지원: openssl >= 1.1.1, libressl >= 3.7)

set -euo pipefail

KEY_PATH=""
MANIFEST=""
ALL_BUILTINS=0

usage() {
    cat <<'USAGE'
Usage:
  sign-bundle.sh --key <private-key.pem> --manifest <path-to-tasty-plugin.toml>
  sign-bundle.sh --key <private-key.pem> --all-builtins
USAGE
    exit "${1:-2}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --key)
            [[ $# -ge 2 ]] || usage 2
            KEY_PATH="$2"
            shift 2
            ;;
        --manifest)
            [[ $# -ge 2 ]] || usage 2
            MANIFEST="$2"
            shift 2
            ;;
        --all-builtins)
            ALL_BUILTINS=1
            shift
            ;;
        -h|--help)
            usage 0
            ;;
        *)
            echo "Error: unknown argument: $1" >&2
            usage 2
            ;;
    esac
done

if [[ -z "$KEY_PATH" ]]; then
    echo "Error: --key is required" >&2
    usage 2
fi

if ! command -v openssl >/dev/null 2>&1; then
    echo "Error: openssl not found in PATH" >&2
    exit 1
fi

if [[ ! -f "$KEY_PATH" ]]; then
    echo "Error: private key not found: $KEY_PATH" >&2
    echo "  Run ./scripts/gen-dev-key.sh to generate a dev key." >&2
    exit 1
fi

# 모드 결정. 단일/전체 동시 지정은 금지.
if [[ -n "$MANIFEST" && "$ALL_BUILTINS" -eq 1 ]]; then
    echo "Error: --manifest and --all-builtins are mutually exclusive" >&2
    usage 2
fi
if [[ -z "$MANIFEST" && "$ALL_BUILTINS" -eq 0 ]]; then
    echo "Error: must specify --manifest or --all-builtins" >&2
    usage 2
fi

# 대상 manifest 목록 수집.
declare -a MANIFESTS=()
if [[ "$ALL_BUILTINS" -eq 1 ]]; then
    REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
    while IFS= read -r -d '' f; do
        MANIFESTS+=("$f")
    done < <(find "$REPO_ROOT/crates" -mindepth 2 -maxdepth 2 -name 'tasty-plugin.toml' -print0)
    if [[ ${#MANIFESTS[@]} -eq 0 ]]; then
        echo "Error: no crates/tasty-plugin-*/tasty-plugin.toml found" >&2
        exit 1
    fi
else
    if [[ ! -f "$MANIFEST" ]]; then
        echo "Error: manifest not found: $MANIFEST" >&2
        exit 1
    fi
    MANIFESTS=("$MANIFEST")
fi

# 임시 디렉토리 (digest 중간 파일). 종료 시 자동 정리.
TMPDIR_SIGN="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_SIGN"' EXIT

SIGNED=0
for manifest in "${MANIFESTS[@]}"; do
    sig_path="${manifest}.sig"
    digest_path="$TMPDIR_SIGN/$(basename "$(dirname "$manifest")").sha256"

    # 1) SHA-256 digest.
    openssl dgst -sha256 -binary "$manifest" > "$digest_path"

    # 2) Ed25519 sign(digest).
    #    -rawin 은 openssl 이 digest 를 그대로 message 로 사용하게 함.
    openssl pkeyutl -sign \
        -inkey "$KEY_PATH" \
        -rawin \
        -in "$digest_path" \
        -out "$sig_path"

    # 3) 결과 길이 검증 (반드시 64 byte).
    sig_len=$(wc -c < "$sig_path" | tr -d ' ')
    if [[ "$sig_len" != "64" ]]; then
        echo "Error: $sig_path is $sig_len bytes, expected 64" >&2
        echo "  (확인 사항: --key 가 Ed25519 키인가? openssl 버전이 1.1.1 이상인가?)" >&2
        rm -f "$sig_path"
        exit 1
    fi

    SIGNED=$((SIGNED + 1))
    echo "  signed: $manifest → $sig_path"
done

echo "==> Signed $SIGNED plugin manifest(s)."
