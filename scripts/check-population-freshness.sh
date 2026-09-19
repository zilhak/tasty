#!/usr/bin/env bash
# 공용 모수 선언이 **그 리비전의 트리**와 맞는지 본다.
#
# `crates/tasty-doc-guards/src/floored_walk.rs` 의 `populations::*` 는 여러 가드가
# 공유하는 좌변이고, 그 `measured` 는 손으로 적힌 수다. 그 수가 지금 트리와 어긋나도
# 어느 가드도 안 묻는다 — `Floor::validate` 는 `min <= measured` 와 날짜 형식만 본다.
#
# ★ 이 판정의 모수는 **리비전 하나의 트리 전체**다. lane 의 작업 트리가 아니다.
#   그것이 이 자리에 있는 이유다: 두 lane 이 각자 파일을 하나씩 더하면 둘 다 자기
#   base 기준으로 +1 을 적고 **같은 값**을 쓴다. 값이 같으므로 git 은 충돌을 안 내고,
#   두 lane 의 검사는 각자의 트리에서 참이며, 병합된 트리에서만 거짓이 된다.
#   그래서 lane 안에서 아무리 정확히 재도 이 형태는 안 잡힌다 — 병합된 트리를 모수로
#   삼는 자리에서만 잡힌다.
#
# 용법:
#   bash scripts/check-population-freshness.sh [--rev <리비전>]
#
# 종료 코드: 0 통과 · 1 위반 · 2 판정 불가(통과가 아니다).
set -u

DECL_PATH="crates/tasty-doc-guards/src/floored_walk.rs"
REV="HEAD"

while [ "$#" -gt 0 ]; do
    case "$1" in
        --rev)
            [ "$#" -ge 2 ] || { echo "[population] --rev 에 리비전이 없다." >&2; exit 2; }
            REV="$2"
            shift 2
            ;;
        *)
            echo "[population] 모르는 인자: $1" >&2
            exit 2
            ;;
    esac
done

if ! git rev-parse --git-dir >/dev/null 2>&1; then
    echo "[population] git 저장소가 아니다 — 모수를 정할 수 없다." >&2
    exit 2
fi
if ! git rev-parse -q --verify "${REV}^{commit}" >/dev/null 2>&1; then
    echo "[population] 리비전 ${REV} 을 못 찾았다 — 모수를 정할 수 없다." >&2
    exit 2
fi

decl_src="$(git show "${REV}:${DECL_PATH}" 2>/dev/null)" || {
    echo "[population] ${REV} 에 ${DECL_PATH} 가 없다 — 모수를 정할 수 없다." >&2
    exit 2
}
tree="$(git ls-tree -r --name-only "$REV")" || {
    echo "[population] ${REV} 의 트리를 못 읽었다." >&2
    exit 2
}

# 선언에서 `pub const <이름>: Population = Population { … measured: <수>` 를 꺼낸다.
declared_of() {
    printf '%s\n' "$decl_src" | awk -v want="$1" '
        $0 ~ ("pub const " want ": Population") { inside = 1; next }
        inside && /measured:/ {
            if (match($0, /measured: *[0-9]+/)) {
                v = substr($0, RSTART, RLENGTH)
                sub(/measured: */, "", v)
                print v
            }
            exit
        }
    '
}

# 트리에서 같은 술어로 센다. 술어는 선언의 `how` 가 글로 적은 것과 같다.
counted_of() {
    case "$1" in
        SRC_RS)             printf '%s\n' "$tree" | grep -cE '^src/.*\.rs$' ;;
        ROOT_TEST_TARGETS)  printf '%s\n' "$tree" | grep -cE '^tests/[^/]+\.rs$' ;;
        CRATE_TEST_TARGETS) printf '%s\n' "$tree" | grep -cE '^crates/[^/]+/tests/[^/]+\.rs$' ;;
        DOCS_MD)            printf '%s\n' "$tree" | grep -cE '^docs/.*\.md$' ;;
        *)                  return 2 ;;
    esac
}

NAMES="SRC_RS ROOT_TEST_TARGETS CRATE_TEST_TARGETS DOCS_MD"
judged=0
bad=0

for name in $NAMES; do
    declared="$(declared_of "$name")"
    if [ -z "$declared" ]; then
        echo "[population] ${REV} 의 선언에서 ${name} 의 measured 를 못 읽었다 — 선언 형태가 바뀌었으면 이 추출기를 고쳐라. 지금은 대조군이 죽은 상태다." >&2
        exit 2
    fi
    counted="$(counted_of "$name")"
    judged=$((judged + 1))
    if [ "$declared" != "$counted" ]; then
        bad=$((bad + 1))
        echo "✘ [population] ${name}: 선언 ${declared} · ${REV} 의 트리 ${counted}" >&2
        echo "    고쳐라: ${DECL_PATH} 의 ${name} 에서 measured 를 ${counted} 로, measured_on 과 counted_on 을 이 트리로 함께 옮겨라." >&2
        echo "    ★ 값만 고치고 끝내지 마라 — 이 모수에서 파생하는 자리의 여유 서술은 수를 안 따라온다." >&2
    fi
done

# 빈 모수를 훑은 초록과 실제로 판정한 초록이 같은 줄로 보이지 않게 한다.
if [ "$bad" -ne 0 ]; then
    echo "[population] 위반 ${bad} 건 / 판정 대상 ${judged} 건 (모수: ${REV} 의 트리)" >&2
    exit 1
fi
echo "[population] 통과 — 판정 대상 ${judged} 건 (모수: ${REV} 의 트리)"
exit 0
