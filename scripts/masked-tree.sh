#!/usr/bin/env bash
# 1 회성 스캔이 읽을 **마스킹 사본**을 만들고 그 경로를 찍는다.
#
# 왜 있나: 소스를 텍스트로 훑어 수를 내는 조사 스크립트는 문자열 리터럴과 주석 안의
# '언급' 을 실물로 센다. 판정 대상 형태를 회귀로 박은 픽스처가 소스에 있으므로 그 오염은
# 예외가 아니라 기본값이다 — 실측으로 여러 번 밟았고, 한 번은 오염된 수가 그대로 근거로
# 굳을 뻔했다. 커밋되는 게이트 넷은 이미 같은 판정기를 쓰는데, 1 회성 스캔은 그 배선을
# 매번 다시 찾아야 해서 안 쓰게 된다. 이 스크립트가 그 한 줄을 대신한다.
#
# 이건 **채널이 아니라 편의**다. 조사 스크립트는 커밋되지 않으므로 아무 게이트도 그것이
# 마스킹을 썼는지 강제할 수 없다. 강제할 수 있는 것은 그 수를 문서에 적을 때뿐이다
# (docs/adr/0139-… 의 계보 분류).
#
# 사용:
#   dir=$(bash scripts/masked-tree.sh)                 # src crates, 주석까지 덮음
#   dir=$(bash scripts/masked-tree.sh --keep-comments) # 주석은 남김
#   dir=$(bash scripts/masked-tree.sh -- src)          # 스캔 루트 지정
#   rg '<패턴>' "$dir"          # 줄 번호는 원본과 같다 — 좌표를 그대로 읽으면 된다
#   rm -rf "$dir"               # 정리는 부른 쪽이 한다(경로를 넘겨야 해서 trap 을 못 건다)
#
# 판정기가 없으면 **짓는다**(의존 0 크레이트라 초 단위). pre-commit 은 이걸 부르면
# 안 된다 — 훅에서 cargo 를 부르면 락 경합으로 수십 초를 기다린다(실측 36.4 s).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
. "$(cd "$(dirname "$0")" && pwd)/lib/judge-bin.sh"

KEEP=""
if [ "${1:-}" = "--keep-comments" ]; then KEEP="--keep-comments"; shift; fi
[ "${1:-}" = "--" ] && shift
if [ "$#" -gt 0 ]; then SCAN_ROOTS=("$@"); else SCAN_ROOTS=(src crates); fi

BIN="$(resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT")"
if [ -z "$BIN" ]; then
    echo "[masked-tree] 판정기를 짓는다 (cargo build -p tasty-doc-guards --bin mask-source)" >&2
    (cd "$ROOT" && cargo build -p tasty-doc-guards --bin mask-source >&2)
    BIN="$(resolve_judge mask-source TASTY_MASK_SOURCE_BIN "$ROOT")"
fi
[ -n "$BIN" ] || { echo "[masked-tree] 판정기를 못 찾았다 — 사본 없이 재지 마라." >&2; exit 2; }

OUT="$(mktemp -d)"
# shellcheck disable=SC2086
if ! "$BIN" $KEEP "$OUT" "$ROOT" "${SCAN_ROOTS[@]}" >&2; then
    rm -rf "$OUT"
    echo "[masked-tree] 사본을 못 만들었다 — 원문에서 재면 픽스처가 실물로 세어진다." >&2
    exit 2
fi
printf '%s\n' "$OUT"
