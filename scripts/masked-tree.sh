#!/usr/bin/env bash
# 조사용 마스킹 사본을 만들고 경로만 stdout으로 반환한다. 사본 삭제는 호출자가 맡는다.
# 사용: dir=$(bash scripts/masked-tree.sh [--keep-comments] [-- src crates])
# 기본은 src와 crates의 문자열·주석을 지우며 원본 줄 번호를 유지한다.
# 최신 mask-source를 찾지 못하면 Cargo 빌드를 시도한다. 중첩 빌드 잠금이 생길 수 있는 훅에서는 호출하지 않는다.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"

KEEP=""
if [ "${1:-}" = "--keep-comments" ]; then KEEP="--keep-comments"; shift; fi
[ "${1:-}" = "--" ] && shift
if [ "$#" -gt 0 ]; then SCAN_ROOTS=("$@"); else SCAN_ROOTS=(src crates); fi

resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
BIN="$JUDGE_BIN"
if [ -z "$BIN" ]; then
    echo "[masked-tree] 마스킹 도구를 빌드한다 (cargo build -p tasty-doc-guards --bin mask-source)" >&2
    (cd "$ROOT" && cargo build -p tasty-doc-guards --bin mask-source >&2)
    resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT"
    BIN="$JUDGE_BIN"
fi
[ -n "$BIN" ] || { echo "[masked-tree] 마스킹 도구를 찾지 못해 중단한다." >&2; exit 2; }

OUT="$(mktemp -d)"
if ! "$BIN" $KEEP "$OUT" "$ROOT" "${SCAN_ROOTS[@]}" >&2; then
    rm -rf "$OUT"
    echo "[masked-tree] 사본을 만들지 못했다. 원문을 대신 세면 합성 입력도 코드로 집계될 수 있다." >&2
    exit 2
fi
printf '%s\n' "$OUT"
