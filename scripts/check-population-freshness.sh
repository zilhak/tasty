#!/usr/bin/env bash
# 지정한 Git revision의 파일 수와 Population.measured 선언을 비교한다. 작업 트리의 미커밋 변경은 세지 않는다.
# 사용: bash scripts/check-population-freshness.sh [--rev revision] (기본 HEAD). 선언 위치·집계 조건은 아래 네 항목을 따른다.
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

# Population 선언의 정해진 줄 형식을 읽는다. Rust 구문 전체를 해석하지 않는다.
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
        echo "[population] ${REV} 의 선언에서 ${name} 의 measured 를 못 읽었다 — 선언 형식이 바뀌었는지 확인해라. 현재는 비교할 값을 얻지 못했다." >&2
        exit 2
    fi
    counted="$(counted_of "$name")"
    judged=$((judged + 1))
    if [ "$declared" != "$counted" ]; then
        bad=$((bad + 1))
        echo "✘ [population] ${name}: 선언 ${declared} · ${REV} 의 트리 ${counted}" >&2
        echo "    고쳐라: ${DECL_PATH} 의 ${name} 에서 measured 를 ${counted} 로, measured_on 과 counted_on 을 이 트리로 함께 옮겨라." >&2
        echo "    이 개수를 인용한 문서의 하한·여유 설명도 함께 확인해라." >&2
    fi
done

if [ "$bad" -ne 0 ]; then
    echo "[population] 위반 ${bad} 건 / 판정 대상 ${judged} 건 (모수: ${REV} 의 트리)" >&2
    exit 1
fi
echo "[population] 통과 — 판정 대상 ${judged} 건 (모수: ${REV} 의 트리)"
exit 0
